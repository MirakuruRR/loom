#pragma once

#include "CoreMinimal.h"
#include "Dom/JsonObject.h"

/**
 * Builds the project's Loom sources into Blueprints: every .lm file under Content is one program,
 * compiled together by the compiler bundled with the plugin, against the types in memory. Each
 * source makes the Blueprint beside it, so Content/Counter/BP_Counter.lm must say it is at
 * /Game/Counter/BP_Counter. The editor runs it when a source changes, and the LoomBuild commandlet
 * runs it headless.
 *
 * The report says what happened, and is saved to Intermediate/Loom/report.json:
 *   ok           whether every source compiled and every Blueprint it makes was built
 *   sources      how many .lm files there were
 *   errors       [{file, line, column, message}], file relative to the project, line and column
 *                only when the compiler gave them
 *   blueprints   [{path, source, status}], status one of built, unchanged, failed or skipped, a
 *                failed one's applier report under "report", and under "uses" the Blueprint a
 *                skipped one uses that was not built
 *
 * A Blueprint that fails does not stop the build: the rest are applied, but one that uses it is
 * skipped, and each failed or skipped one has an error of its own.
 */
struct LOOMEDITOR_API FLoomBuild
{
	/**
	 * Builds each Blueprint whose build record, saved with it, differs from what it would be built
	 * from: its ops and the version of Loom. Force builds every one.
	 */
	static TSharedRef<FJsonObject> Run(bool bForce);

	static FString ToText(const TSharedRef<FJsonObject>& Report);

	/** Where the plugin carries one of its programs for this platform: loom, or loom-mcp. */
	static FString ProgramPath(const FString& Name);
};
