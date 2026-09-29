#pragma once

#include "Commandlets/Commandlet.h"
#include "LoomTestCommandlet.generated.h"

/**
 * UnrealEditor-Cmd <project> -run=LoomTest -test=<test.json> -report=<report.json>
 * Calls functions of a compiled Blueprint on its class default object, checks the values they
 * leave, and times repeated calls. Returns 0 when every case passed.
 */
UCLASS()
class ULoomTestCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	virtual int32 Main(const FString& Params) override;
};
