#pragma once

#include "Containers/Ticker.h"
#include "Dom/JsonObject.h"
#include "IDirectoryWatcher.h"
#include "Modules/ModuleInterface.h"
#include "Styling/SlateStyle.h"

/**
 * In the editor, builds the project's Loom sources whenever one under Content is saved, and from
 * Tools > Rebuild All Blueprints. Errors go to the "Loom" message log. Tools > Connect an Agent shows
 * how to give an agent Loom's MCP server.
 */
class FLoomEditorModule : public IModuleInterface
{
public:
	virtual void StartupModule() override;
	virtual void ShutdownModule() override;

	/** Runs FLoomBuild and shows what it said. */
	static TSharedRef<FJsonObject> BuildAndShow(bool bForce);

	/** Asks loom-mcp how to add it to an agent, and shows that in a window. */
	static void ShowConnectAgent();

private:
	void OnContentChanged(const TArray<FFileChangeData>& Changes);

	FString WatchedDir;
	FDelegateHandle WatchHandle;
	FTSTicker::FDelegateHandle PendingBuild;
	/** The menu items' icons that Unreal's own styles don't name. */
	TSharedPtr<FSlateStyleSet> Style;
};
