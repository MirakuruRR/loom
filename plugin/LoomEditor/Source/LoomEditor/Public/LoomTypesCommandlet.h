#pragma once

#include "Commandlets/Commandlet.h"
#include "LoomTypesCommandlet.generated.h"

/**
 * UnrealEditor-Cmd <project> -run=LoomTypes -report=<types.json>
 * Dumps every loaded class's Blueprint-visible properties and callable functions, and the structs
 * and enums they name, for the compiler to type-check against.
 */
UCLASS()
class ULoomTypesCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	virtual int32 Main(const FString& Params) override;
};
