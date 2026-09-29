#pragma once

#include "Commandlets/Commandlet.h"
#include "LoomBuildCommandlet.generated.h"

/**
 * UnrealEditor-Cmd <project> -run=LoomBuild [-report=<report.json>] [-force]
 * Builds the project's Loom sources into Blueprints, as the editor does when one changes, and
 * exits 0 when every one was built. A Blueprint whose build record matches what it would be built
 * from is left alone, unless -force. The report is FLoomBuild's.
 */
UCLASS()
class ULoomBuildCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	virtual int32 Main(const FString& Params) override;
};
