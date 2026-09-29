#pragma once

#include "CoreMinimal.h"
#include "Engine/DeveloperSettings.h"
#include "LoomSettings.generated.h"

/** The project's Loom settings, in Project Settings > Plugins > Loom, saved to Config/DefaultLoom.ini. */
UCLASS(config = Loom, defaultconfig, meta = (DisplayName = "Loom"))
class LOOMEDITOR_API ULoomSettings : public UDeveloperSettings
{
	GENERATED_BODY()

public:
	ULoomSettings()
	{
		CategoryName = TEXT("Plugins");
	}

	/**
	 * The folders whose Blueprint classes a Loom Blueprint may hold as components and widgets, such
	 * as /Game/Components. Native classes are always allowed. Empty allows any class. A project
	 * whose Blueprint classes include stand-ins for classes defined elsewhere, which would be saved
	 * with the wrong property layout, lists only the folders whose classes are real.
	 */
	UPROPERTY(config, EditAnywhere, Category = "Blueprints")
	TArray<FString> InstancedClassPaths;
};
