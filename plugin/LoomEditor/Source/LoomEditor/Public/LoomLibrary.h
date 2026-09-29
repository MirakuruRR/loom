#pragma once

#include "Kismet/BlueprintFunctionLibrary.h"
#include "LoomLibrary.generated.h"

/** Loom's build, for editor scripts: unreal.LoomLibrary.build_blueprints() in Python. */
UCLASS()
class ULoomLibrary : public UBlueprintFunctionLibrary
{
	GENERATED_BODY()

public:
	/**
	 * Builds the Blueprints made from the project's Loom files, as saving one does, and returns the
	 * report as JSON (see FLoomBuild). Force builds every Blueprint, even one whose source has not
	 * changed.
	 */
	UFUNCTION(BlueprintCallable, Category = "Loom")
	static FString BuildBlueprints(bool bForce = true);
};
