#include "LoomEditorModule.h"

#include "Brushes/SlateImageBrush.h"
#include "DirectoryWatcherModule.h"
#include "Framework/Application/SlateApplication.h"
#include "Framework/Notifications/NotificationManager.h"
#include "HAL/PlatformApplicationMisc.h"
#include "HAL/PlatformProcess.h"
#include "LoomBuild.h"
#include "Logging/MessageLog.h"
#include "MessageLogModule.h"
#include "Misc/MessageDialog.h"
#include "Misc/Paths.h"
#include "Modules/ModuleManager.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"
#include "Styling/AppStyle.h"
#include "Styling/CoreStyle.h"
#include "Styling/SlateStyleRegistry.h"
#include "ToolMenus.h"
#include "Widgets/Input/SButton.h"
#include "Widgets/Input/SMultiLineEditableTextBox.h"
#include "Widgets/Layout/SBorder.h"
#include "Widgets/Notifications/SNotificationList.h"
#include "Widgets/SWindow.h"

#define LOCTEXT_NAMESPACE "LoomEditor"

IMPLEMENT_MODULE(FLoomEditorModule, LoomEditor)

namespace
{
	const FName LogName(TEXT("Loom"));
	constexpr float Settle = 0.5f;
}

void FLoomEditorModule::StartupModule()
{
	// Commandlets build when they are told to, with LoomBuild.
	if (IsRunningCommandlet() || !GIsEditor)
	{
		return;
	}
	FMessageLogModule& MessageLog = FModuleManager::LoadModuleChecked<FMessageLogModule>(TEXT("MessageLog"));
	MessageLog.RegisterLogListing(LogName, LOCTEXT("LogLabel", "Loom"));

	// The plug the editor's Plugins window uses, which no style names. The file is the engine's
	// own, used where it is rather than copied into the plugin.
	Style = MakeShared<FSlateStyleSet>(TEXT("LoomEditorStyle"));
	Style->SetContentRoot(FPaths::EngineContentDir() / TEXT("Editor/Slate"));
	Style->Set("Loom.ConnectAgent", new FSlateVectorImageBrush(Style->RootToContentDir(TEXT("Starship/Common/plugins"), TEXT(".svg")), CoreStyleConstants::Icon16x16));
	FSlateStyleRegistry::RegisterSlateStyle(*Style);

	WatchedDir = FPaths::ConvertRelativePathToFull(FPaths::ProjectContentDir());
	FDirectoryWatcherModule& Watcher = FModuleManager::LoadModuleChecked<FDirectoryWatcherModule>(TEXT("DirectoryWatcher"));
	Watcher.Get()->RegisterDirectoryChangedCallback_Handle(WatchedDir,
		IDirectoryWatcher::FDirectoryChanged::CreateRaw(this, &FLoomEditorModule::OnContentChanged), WatchHandle);

	UToolMenus::RegisterStartupCallback(FSimpleMulticastDelegate::FDelegate::CreateLambda([this]
	{
		FToolMenuOwnerScoped Owner(this);
		FToolMenuSection& Section = UToolMenus::Get()->ExtendMenu(TEXT("LevelEditor.MainMenu.Tools"))
			->FindOrAddSection(TEXT("Loom"), LOCTEXT("MenuSection", "Loom"));
		Section.AddMenuEntry(TEXT("LoomBuild"), LOCTEXT("BuildLabel", "Rebuild All Blueprints"),
			LOCTEXT("BuildTooltip", "Rebuild every Blueprint made from a Loom file, even unchanged ones."),
			FSlateIcon(FAppStyle::GetAppStyleSetName(), "Icons.Refresh"), FUIAction(FExecuteAction::CreateLambda([] { BuildAndShow(true); })));
		Section.AddMenuEntry(TEXT("LoomConnectAgent"), LOCTEXT("ConnectLabel", "Connect an Agent..."),
			LOCTEXT("ConnectTooltip", "Show how to give a coding agent, such as Claude Code, Loom's MCP server for this project."),
			FSlateIcon(Style->GetStyleSetName(), "Loom.ConnectAgent"), FUIAction(FExecuteAction::CreateStatic(&FLoomEditorModule::ShowConnectAgent)));
	}));
}

void FLoomEditorModule::ShutdownModule()
{
	if (WatchHandle.IsValid())
	{
		if (FDirectoryWatcherModule* Watcher = FModuleManager::GetModulePtr<FDirectoryWatcherModule>(TEXT("DirectoryWatcher")))
		{
			Watcher->Get()->UnregisterDirectoryChangedCallback_Handle(WatchedDir, WatchHandle);
		}
	}
	if (PendingBuild.IsValid())
	{
		FTSTicker::GetCoreTicker().RemoveTicker(PendingBuild);
	}
	if (UObjectInitialized())
	{
		UToolMenus::UnRegisterStartupCallback(this);
		UToolMenus::UnregisterOwner(this);
	}
	if (Style.IsValid())
	{
		FSlateStyleRegistry::UnRegisterSlateStyle(*Style);
		Style.Reset();
	}
}

void FLoomEditorModule::OnContentChanged(const TArray<FFileChangeData>& Changes)
{
	const bool bSource = Changes.ContainsByPredicate([](const FFileChangeData& Change) { return Change.Filename.EndsWith(TEXT(".lm")); });
	if (!bSource || PendingBuild.IsValid())
	{
		return;
	}
	PendingBuild = FTSTicker::GetCoreTicker().AddTicker(FTickerDelegate::CreateLambda([this](float)
	{
		PendingBuild.Reset();
		BuildAndShow(false);
		return false;
	}), Settle);
}

TSharedRef<FJsonObject> FLoomEditorModule::BuildAndShow(bool bForce)
{
	const TSharedRef<FJsonObject> Report = FLoomBuild::Run(bForce);
	FMessageLog Log(LogName);
	const TArray<TSharedPtr<FJsonValue>>& Errors = Report->GetArrayField(TEXT("errors"));
	if (!Errors.IsEmpty())
	{
		Log.NewPage(LOCTEXT("Page", "Build"));
	}
	for (const TSharedPtr<FJsonValue>& Value : Errors)
	{
		const TSharedPtr<FJsonObject>& Error = Value->AsObject();
		FString Where = Error->GetStringField(TEXT("file"));
		int32 Line = 0;
		if (Error->TryGetNumberField(TEXT("line"), Line))
		{
			Where += FString::Printf(TEXT(":%d:%d"), Line, (int32)Error->GetNumberField(TEXT("column")));
		}
		Log.Error(FText::FromString(Where.IsEmpty() ? Error->GetStringField(TEXT("message"))
			: Where + TEXT(": ") + Error->GetStringField(TEXT("message"))));
	}
	if (!Report->GetBoolField(TEXT("ok")))
	{
		Log.Notify(LOCTEXT("Failed", "Loom: the build failed"));
		return Report;
	}
	int32 Built = 0;
	for (const TSharedPtr<FJsonValue>& Value : Report->GetArrayField(TEXT("blueprints")))
	{
		Built += Value->AsObject()->GetStringField(TEXT("status")) == TEXT("built");
	}
	if (Built > 0)
	{
		FNotificationInfo Info(FText::Format(LOCTEXT("Built", "Loom: built {0} {0}|plural(one=Blueprint,other=Blueprints)"), Built));
		Info.ExpireDuration = 3.0f;
		FSlateNotificationManager::Get().AddNotification(Info);
	}
	return Report;
}

void FLoomEditorModule::ShowConnectAgent()
{
	// What to show comes from loom-mcp itself, which says the same when run from a terminal.
	const FString Program = FLoomBuild::ProgramPath(TEXT("loom-mcp"));
	const FString Params = FString::Printf(TEXT("--setup --json --project \"%s\""),
		*FPaths::ConvertRelativePathToFull(FPaths::ProjectDir()));
	int32 Code = -1;
	FString Stdout;
	FString Stderr;
	TSharedPtr<FJsonObject> Setup;
	FString Error;
	if (!FPaths::FileExists(Program))
	{
		Error = FString::Printf(TEXT("Loom's MCP server is missing from the plugin: no %s"), *Program);
	}
	else if (!FPlatformProcess::ExecProcess(*Program, *Params, &Code, &Stdout, &Stderr))
	{
		Error = FString::Printf(TEXT("cannot run %s"), *Program);
	}
	else if (Code != 0 || !FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Stdout), Setup) || !Setup.IsValid())
	{
		Error = FString::Printf(TEXT("%s failed (exit %d): %s"), *Program, Code, *Stderr.TrimStartAndEnd());
	}
	if (!Error.IsEmpty())
	{
		FMessageDialog::Open(EAppMsgType::Ok, FText::FromString(Error), LOCTEXT("ConnectTitle", "Connect an Agent"));
		return;
	}

	const FString Command = Setup->GetStringField(TEXT("claude"));
	const TSharedRef<SWindow> Window = SNew(SWindow)
		.Title(LOCTEXT("ConnectTitle", "Connect an Agent"))
		.ClientSize(FVector2D(820, 460))
		.SupportsMinimize(false)
		.SupportsMaximize(false);
	const TWeakPtr<SWindow> WeakWindow = Window;
	Window->SetContent(
		SNew(SBorder)
		.BorderImage(FAppStyle::GetBrush(TEXT("ToolPanel.GroupBorder")))
		.Padding(16.0f)
		[
			SNew(SVerticalBox)
			+ SVerticalBox::Slot()
			.FillHeight(1.0f)
			[
				SNew(SMultiLineEditableTextBox)
				.Text(FText::FromString(Setup->GetStringField(TEXT("text"))))
				.Font(FCoreStyle::GetDefaultFontStyle(TEXT("Mono"), 10))
				.IsReadOnly(true)
			]
			+ SVerticalBox::Slot()
			.AutoHeight()
			.HAlign(HAlign_Right)
			.Padding(0.0f, 12.0f, 0.0f, 0.0f)
			[
				SNew(SHorizontalBox)
				+ SHorizontalBox::Slot()
				.AutoWidth()
				[
					SNew(SButton)
					.Text(LOCTEXT("CopyCommand", "Copy the Claude Code command"))
					.OnClicked_Lambda([Command]
					{
						FPlatformApplicationMisc::ClipboardCopy(*Command);
						return FReply::Handled();
					})
				]
				+ SHorizontalBox::Slot()
				.AutoWidth()
				.Padding(8.0f, 0.0f, 0.0f, 0.0f)
				[
					SNew(SButton)
					.Text(LOCTEXT("Close", "Close"))
					.OnClicked_Lambda([WeakWindow]
					{
						if (const TSharedPtr<SWindow> Pinned = WeakWindow.Pin())
						{
							Pinned->RequestDestroyWindow();
						}
						return FReply::Handled();
					})
				]
			]
		]);
	FSlateApplication::Get().AddWindow(Window);
}

#undef LOCTEXT_NAMESPACE
