#include "LoomBuild.h"

#include "Editor.h"
#include "Engine/Blueprint.h"
#include "HAL/FileManager.h"
#include "HAL/PlatformProcess.h"
#include "Interfaces/IPluginManager.h"
#include "LoomApplier.h"
#include "LoomTypes.h"
#include "Misc/FileHelper.h"
#include "Misc/PackageName.h"
#include "Misc/Paths.h"
#include "Misc/SecureHash.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"
#include "Subsystems/AssetEditorSubsystem.h"

DEFINE_LOG_CATEGORY_STATIC(LogLoomBuild, Log, All);

namespace
{
	void AddError(TArray<TSharedPtr<FJsonValue>>& Errors, const FString& File, int32 Line, int32 Column, const FString& Message)
	{
		TSharedRef<FJsonObject> Error = MakeShared<FJsonObject>();
		Error->SetStringField(TEXT("file"), File);
		if (Line > 0)
		{
			Error->SetNumberField(TEXT("line"), Line);
			Error->SetNumberField(TEXT("column"), Column);
		}
		Error->SetStringField(TEXT("message"), Message);
		Errors.Add(MakeShared<FJsonValueObject>(Error));
	}

	/**
	 * Runs the compiler on the project, with --json, and reads what it printed. A command that could
	 * not run, or printed no JSON, is an error of its own.
	 */
	TSharedPtr<FJsonValue> RunCompiler(const FString& Compiler, const FString& Params, TArray<TSharedPtr<FJsonValue>>& Errors)
	{
		int32 Code = -1;
		FString Stdout;
		FString Stderr;
		if (!FPlatformProcess::ExecProcess(*Compiler, *Params, &Code, &Stdout, &Stderr))
		{
			AddError(Errors, FString(), 0, 0, FString::Printf(TEXT("cannot run %s"), *Compiler));
			return nullptr;
		}
		TSharedPtr<FJsonValue> Result;
		if (!FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Stdout), Result) || !Result.IsValid())
		{
			const FString Said = Stderr.TrimStartAndEnd();
			AddError(Errors, FString(), 0, 0, FString::Printf(TEXT("the compiler failed (exit %d): %s"), Code,
				Said.IsEmpty() ? *Stdout.TrimStartAndEnd() : *Said));
			return nullptr;
		}
		return Result;
	}

	/** Why an applier failed: its own error, the failing op's, or the Blueprint compiler's. */
	FString ApplyError(const TSharedRef<FJsonObject>& Report)
	{
		FString Error;
		if (Report->TryGetStringField(TEXT("error"), Error))
		{
			return Error;
		}
		const TArray<TSharedPtr<FJsonValue>>* Ops = nullptr;
		if (Report->TryGetArrayField(TEXT("ops"), Ops))
		{
			for (const TSharedPtr<FJsonValue>& Op : *Ops)
			{
				const TSharedPtr<FJsonObject>& Outcome = Op->AsObject();
				if (Outcome.IsValid() && Outcome->TryGetStringField(TEXT("error"), Error))
				{
					return FString::Printf(TEXT("op %d (%s): %s"), (int32)Outcome->GetNumberField(TEXT("index")),
						*Outcome->GetStringField(TEXT("op")), *Error);
				}
			}
		}
		return TEXT("the Blueprint did not compile cleanly");
	}
}

TSharedRef<FJsonObject> FLoomBuild::Run(bool bForce)
{
	TSharedRef<FJsonObject> Report = MakeShared<FJsonObject>();
	TArray<TSharedPtr<FJsonValue>> Errors;
	TArray<TSharedPtr<FJsonValue>> Blueprints;
	auto Finish = [&](bool bOk)
	{
		Report->SetBoolField(TEXT("ok"), bOk && Errors.IsEmpty());
		Report->SetArrayField(TEXT("errors"), Errors);
		Report->SetArrayField(TEXT("blueprints"), Blueprints);
		UE_LOG(LogLoomBuild, Display, TEXT("LoomBuild: %s, %d errors, %d Blueprints"),
			Report->GetBoolField(TEXT("ok")) ? TEXT("ok") : TEXT("failed"), Errors.Num(), Blueprints.Num());
		// The last build's report, for whatever reads it without asking the editor, such as an agent.
		const FString ReportPath = FPaths::ProjectIntermediateDir() / TEXT("Loom/report.json");
		if (!FFileHelper::SaveStringToFile(ToText(Report), *ReportPath, FFileHelper::EEncodingOptions::ForceUTF8WithoutBOM))
		{
			UE_LOG(LogLoomBuild, Warning, TEXT("LoomBuild: cannot write %s"), *ReportPath);
		}
		return Report;
	};

	const FString ProjectDir = FPaths::ConvertRelativePathToFull(FPaths::ProjectDir());
	const FString Compiler = ProgramPath(TEXT("loom"));
	if (!FPaths::FileExists(Compiler))
	{
		AddError(Errors, FString(), 0, 0, FString::Printf(TEXT("the Loom compiler is missing from the plugin: no %s"), *Compiler));
		return Finish(false);
	}

	// The compiler finds the sources, every .lm under Content, and the Blueprint each builds.
	const TSharedPtr<FJsonValue> Found = RunCompiler(Compiler, FString::Printf(TEXT("sources --project \"%s\" --json"), *ProjectDir), Errors);
	const TArray<TSharedPtr<FJsonValue>>* Sources = nullptr;
	if (!Found.IsValid() || !Found->TryGetArray(Sources))
	{
		if (Errors.IsEmpty())
		{
			AddError(Errors, FString(), 0, 0, TEXT("the compiler listed no sources"));
		}
		return Finish(false);
	}
	Report->SetNumberField(TEXT("sources"), Sources->Num());

	TSet<FName> Own;
	TSet<FString> Roots;
	for (const TSharedPtr<FJsonValue>& Source : *Sources)
	{
		const TSharedPtr<FJsonObject>& Object = Source->AsObject();
		const FString Package = Object->GetStringField(TEXT("package"));
		Own.Add(FName(Package));
		// Blueprints the sources may use are loaded from the folders they are in and the folders
		// they use, and nowhere else: a project such as a mod kit holds other mods' Blueprints,
		// which may not even load.
		Roots.Add(FPackageName::GetLongPackagePath(Package));
		const TArray<TSharedPtr<FJsonValue>>* Uses = nullptr;
		if (Object->TryGetArrayField(TEXT("uses"), Uses))
		{
			for (const TSharedPtr<FJsonValue>& Use : *Uses)
			{
				Roots.Add(Use->AsString());
			}
		}
	}

	// The program's own Blueprints are left out of the types: the compiler declares them itself.
	int32 Loaded = 0;
	FString LoadError;
	if (!FLoomTypes::LoadBlueprints(Roots.Array(), Own, Loaded, LoadError))
	{
		AddError(Errors, FString(), 0, 0, LoadError);
		return Finish(false);
	}
	// Where the compiler reads the types, which `loom check` and `loom types` read too.
	const FString WorkDir = FPaths::ConvertRelativePathToFull(FPaths::ProjectIntermediateDir() / TEXT("Loom"));
	const FString TypesPath = WorkDir / TEXT("types.json");
	const FString OpsDir = WorkDir / TEXT("ops");
	auto SaveTypes = [&]()
	{
		if (!FFileHelper::SaveStringToFile(ToText(FLoomTypes::Dump(Own)), *TypesPath, FFileHelper::EEncodingOptions::ForceUTF8WithoutBOM))
		{
			AddError(Errors, FString(), 0, 0, FString::Printf(TEXT("cannot write %s"), *TypesPath));
			return false;
		}
		return true;
	};
	// A project with nothing to build still has its types saved, for `loom check` to write its
	// first source against.
	if (Sources->IsEmpty())
	{
		return Finish(SaveTypes());
	}
	// A source may use a Blueprint outside the folders loaded above, such as one of a game's cooked
	// classes. The compiler names the types it could not find, and while any of them can be
	// loaded, they are, and the program is compiled again with the types they add.
	TSharedPtr<FJsonObject> Result;
	while (true)
	{
		IFileManager::Get().DeleteDirectory(*OpsDir, false, true);
		if (!SaveTypes())
		{
			return Finish(false);
		}
		// Its errors are already the report's: a file relative to the project, a place, a message.
		const TSharedPtr<FJsonValue> Built = RunCompiler(Compiler,
			FString::Printf(TEXT("build --project \"%s\" --out-dir \"%s\" --json"), *ProjectDir, *OpsDir), Errors);
		const TSharedPtr<FJsonObject>* Object = nullptr;
		if (!Built.IsValid() || !Built->TryGetObject(Object))
		{
			return Finish(false);
		}
		Result = *Object;
		TArray<FString> Missing;
		const TArray<TSharedPtr<FJsonValue>>* MissingValues = nullptr;
		if (!Result->GetBoolField(TEXT("ok")) && Result->TryGetArrayField(TEXT("missing"), MissingValues))
		{
			for (const TSharedPtr<FJsonValue>& Value : *MissingValues)
			{
				Missing.Add(Value->AsString());
			}
		}
		if (Missing.IsEmpty() || FLoomTypes::LoadNamed(Missing, Own) == 0)
		{
			break;
		}
	}
	const TArray<TSharedPtr<FJsonValue>>* CompileErrors = nullptr;
	if (Result->TryGetArrayField(TEXT("errors"), CompileErrors))
	{
		Errors.Append(*CompileErrors);
	}
	if (!Result->GetBoolField(TEXT("ok")))
	{
		if (Errors.IsEmpty())
		{
			AddError(Errors, FString(), 0, 0, TEXT("the compiler failed, and said no more"));
		}
		return Finish(false);
	}

	// Each Blueprint after the ones it uses, as the compiler ordered them.
	FString ManifestText;
	TSharedPtr<FJsonObject> Manifest;
	const TArray<TSharedPtr<FJsonValue>>* Order = nullptr;
	if (!FFileHelper::LoadFileToString(ManifestText, *(OpsDir / TEXT("build.json")))
		|| !FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(ManifestText), Manifest) || !Manifest.IsValid()
		|| !Manifest->TryGetArrayField(TEXT("order"), Order))
	{
		AddError(Errors, FString(), 0, 0, TEXT("the compiler wrote no build.json"));
		return Finish(false);
	}
	FString Version;
	if (!Manifest->TryGetStringField(TEXT("version"), Version))
	{
		AddError(Errors, FString(), 0, 0, TEXT("the compiler's build.json has no version"));
		return Finish(false);
	}

	struct FDoc
	{
		FString Path;
		/** The source, relative to the project. */
		FString File;
		FString Text;
		TSharedPtr<FJsonObject> Json;
	};
	TArray<FDoc> Docs;
	for (const TSharedPtr<FJsonValue>& Entry : *Order)
	{
		FDoc& Doc = Docs.AddDefaulted_GetRef();
		const FString OpsPath = OpsDir / Entry->AsString();
		const TArray<TSharedPtr<FJsonValue>>* Ops = nullptr;
		const TSharedPtr<FJsonObject>* First = nullptr;
		if (!FFileHelper::LoadFileToString(Doc.Text, *OpsPath)
			|| !FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Doc.Text), Doc.Json) || !Doc.Json.IsValid()
			|| !Doc.Json->TryGetArrayField(TEXT("ops"), Ops) || Ops->IsEmpty() || !(*Ops)[0]->TryGetObject(First)
			|| !(*First)->TryGetStringField(TEXT("path"), Doc.Path))
		{
			AddError(Errors, FString(), 0, 0, FString::Printf(TEXT("cannot read the compiler's %s"), *OpsPath));
			return Finish(false);
		}
		(*First)->TryGetStringField(TEXT("source"), Doc.File);
	}

	// The Blueprints this build failed or skipped. One that uses any of them is skipped too, rather
	// than applied against an asset that is half made or out of date.
	TArray<FString> NotBuilt;
	for (const FDoc& Doc : Docs)
	{
		TSharedRef<FJsonObject> Entry = MakeShared<FJsonObject>();
		Entry->SetStringField(TEXT("path"), Doc.Path);
		Entry->SetStringField(TEXT("source"), Doc.File);
		Blueprints.Add(MakeShared<FJsonValueObject>(Entry));
		// What it would be built from: its ops, and the version of Loom that made them, which also
		// covers a change in how the plugin applies them. A Blueprint saved with the same record
		// is left alone, so a build rewrites only the Blueprints whose sources changed.
		FTCHARToUTF8 Utf8(*Doc.Text);
		const FString Record = FString::Printf(TEXT("%s %s"), *Version, *FMD5::HashBytes(reinterpret_cast<const uint8*>(Utf8.Get()), Utf8.Length()));
		const FString Object = Doc.Path + TEXT(".") + FPackageName::GetLongPackageAssetName(Doc.Path);
		const UObject* Existing = FPackageName::DoesPackageExist(Doc.Path) ? LoadObject<UObject>(nullptr, *Object) : nullptr;
		if (!bForce && Existing && FLoomApplier::ReadBuildRecord(Existing) == Record)
		{
			Entry->SetStringField(TEXT("status"), TEXT("unchanged"));
			continue;
		}
		// Its ops name another asset by its object path, as the compiler finds what it uses.
		const FString* Uses = NotBuilt.FindByPredicate([&Doc](const FString& Path) { return Doc.Text.Contains(Path + TEXT(".")); });
		if (Uses)
		{
			Entry->SetStringField(TEXT("status"), TEXT("skipped"));
			Entry->SetStringField(TEXT("uses"), *Uses);
			AddError(Errors, Doc.File, 0, 0, FString::Printf(TEXT("not built, since it uses %s, which was not built"), **Uses));
			NotBuilt.Add(Doc.Path);
			continue;
		}
		// An open editor would be left showing graphs or values the build replaces.
		if (GEditor)
		{
			if (UObject* Open = FindObject<UObject>(nullptr, *Object))
			{
				GEditor->GetEditorSubsystem<UAssetEditorSubsystem>()->CloseAllEditorsForAsset(Open);
			}
		}
		FLoomApplier Applier;
		Applier.BuildRecord = Record;
		if (!Applier.Apply(Doc.Json.ToSharedRef()))
		{
			Entry->SetStringField(TEXT("status"), TEXT("failed"));
			Entry->SetObjectField(TEXT("report"), Applier.GetReport());
			AddError(Errors, Doc.File, 0, 0, ApplyError(Applier.GetReport()));
			// A save that failed leaves the record on the asset in memory, where the next build
			// would take it as built.
			if (UObject* Made = FindObject<UObject>(nullptr, *Object); Made && FLoomApplier::ReadBuildRecord(Made) == Record)
			{
				Made->GetPackage()->GetMetaData().RemoveValue(Made, TEXT("LoomBuild"));
			}
			NotBuilt.Add(Doc.Path);
			continue;
		}
		Entry->SetStringField(TEXT("status"), TEXT("built"));
	}
	return Finish(true);
}

FString FLoomBuild::ToText(const TSharedRef<FJsonObject>& Report)
{
	FString Text;
	FJsonSerializer::Serialize(Report, TJsonWriterFactory<TCHAR, TCondensedJsonPrintPolicy<TCHAR>>::Create(&Text));
	return Text;
}

FString FLoomBuild::ProgramPath(const FString& Name)
{
	const TSharedPtr<IPlugin> Plugin = IPluginManager::Get().FindPlugin(TEXT("LoomEditor"));
	check(Plugin);
#if PLATFORM_WINDOWS
	const FString File = TEXT("Win64/") + Name + TEXT(".exe");
#elif PLATFORM_MAC
	const FString File = TEXT("Mac/") + Name;
#else
	const FString File = TEXT("Linux/") + Name;
#endif
	return FPaths::ConvertRelativePathToFull(Plugin->GetBaseDir() / TEXT("Binaries/ThirdParty/Loom") / File);
}
