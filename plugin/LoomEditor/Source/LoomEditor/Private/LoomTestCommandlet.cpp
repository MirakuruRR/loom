#include "LoomTestCommandlet.h"

#include "Engine/Blueprint.h"
#include "Engine/Engine.h"
#include "Engine/World.h"
#include "GameFramework/Actor.h"
#include "HAL/PlatformTime.h"
#include "Misc/FileHelper.h"
#include "Misc/PackageName.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"
#include "UObject/Stack.h"

DEFINE_LOG_CATEGORY_STATIC(LogLoomTest, Log, All);

namespace
{
	/** Collects what the Blueprint VM logs while a case runs, such as a runaway loop. */
	class FScriptMessages : public FOutputDevice
	{
	public:
		TArray<FString> Lines;

		virtual void Serialize(const TCHAR* Text, ELogVerbosity::Type Verbosity, const FName& Category) override
		{
			if (Category == TEXT("LogScript") && Verbosity <= ELogVerbosity::Warning)
			{
				Lines.Add(Text);
			}
		}
	};

	/** Writes each field of Values into the matching property of the struct or object at Container. */
	bool ImportFields(const UStruct* Struct, void* Container, UObject* Owner, const TSharedPtr<FJsonObject>& Values,
		FString& OutError)
	{
		if (!Values.IsValid())
		{
			return true;
		}
		for (const auto& Field : Values->Values)
		{
			const FProperty* Property = Struct->FindPropertyByName(FName(Field.Key));
			if (!Property)
			{
				OutError = FString::Printf(TEXT("%s has no %s"), *Struct->GetName(), *Field.Key);
				return false;
			}
			const FString Text = Field.Value->AsString();
			if (!Property->ImportText_Direct(*Text, Property->ContainerPtrToValuePtr<void>(Container), Owner, PPF_None))
			{
				OutError = FString::Printf(TEXT("cannot read '%s' as %s"), *Text, *Field.Key);
				return false;
			}
		}
		return true;
	}

	FString ExportField(const FProperty* Property, const void* Container)
	{
		FString Out;
		Property->ExportTextItem_Direct(Out, Property->ContainerPtrToValuePtr<void>(Container), nullptr, nullptr, PPF_None);
		return Out;
	}

	/**
	 * Runs one case on Target: set its fields, call the function Repeat times, tick its world
	 * Ticks times, and compare what it expects. A key "return" in expect reads the function's
	 * return value, and a key that names no field of the class reads that parameter as the call
	 * left it.
	 */
	TSharedRef<FJsonObject> RunCase(UObject* Target, UWorld* World, const TSharedRef<FJsonObject>& Case)
	{
		TSharedRef<FJsonObject> Result = MakeShared<FJsonObject>();
		FString Name = TEXT("?");
		Case->TryGetStringField(TEXT("name"), Name);
		Result->SetStringField(TEXT("name"), Name);
		auto Fail = [&Result](const FString& Error)
		{
			Result->SetBoolField(TEXT("ok"), false);
			Result->SetStringField(TEXT("error"), Error);
			return Result;
		};

		UClass* Class = Target->GetClass();
		const FString FunctionName = Case->GetStringField(TEXT("call"));
		UFunction* Function = Class->FindFunctionByName(FName(FunctionName));
		if (!Function)
		{
			return Fail(FString::Printf(TEXT("%s has no function %s"), *Class->GetName(), *FunctionName));
		}
		int32 Repeat = 1;
		Case->TryGetNumberField(TEXT("repeat"), Repeat);
		const TSharedPtr<FJsonObject>* Set = nullptr;
		Case->TryGetObjectField(TEXT("set"), Set);
		const TSharedPtr<FJsonObject>* Args = nullptr;
		Case->TryGetObjectField(TEXT("args"), Args);
		int32 Ticks = 0;
		Case->TryGetNumberField(TEXT("ticks"), Ticks);
		double Dt = 0.1;
		Case->TryGetNumberField(TEXT("dt"), Dt);
		if (Ticks > 0 && !World)
		{
			return Fail(TEXT("ticks need a world to tick; give the test \"world\": true"));
		}

		// The whole function struct, not only its parameters: initialising it initialises the
		// locals too, which sit after the parameters.
		uint8* Params = static_cast<uint8*>(FMemory::Malloc(FMath::Max<int32>(Function->GetStructureSize(), 1), Function->GetMinAlignment()));
		Function->InitializeStruct(Params);
		FString Error;
		double Seconds = 0;
		bool bRan = true;
		FScriptMessages Messages;
		GLog->AddOutputDevice(&Messages);
		for (int32 Call = 0; Call < Repeat && bRan; ++Call)
		{
			if (!ImportFields(Class, Target, Target, Set ? *Set : nullptr, Error)
				|| !ImportFields(Function, Params, Target, Args ? *Args : nullptr, Error))
			{
				bRan = false;
				break;
			}
			// The VM's runaway loop counter is reset once per world tick. Each call stands for a
			// frame of its own, so it gets the whole budget.
			GInitRunaway();
			const double Start = FPlatformTime::Seconds();
			Target->ProcessEvent(Function, Params);
			Seconds += FPlatformTime::Seconds() - Start;
		}
		// Time passes in the world, so its timers fire, each tick a frame of its own: the timer
		// manager ticks once a frame, and the runaway loop counter is reset once a frame.
		for (int32 Tick = 0; Tick < Ticks && bRan; ++Tick)
		{
			++GFrameCounter;
			GInitRunaway();
			World->Tick(LEVELTICK_All, Dt);
		}
		GLog->RemoveOutputDevice(&Messages);

		TSharedRef<FJsonObject> Got = MakeShared<FJsonObject>();
		bool bOk = bRan;
		const TSharedPtr<FJsonObject>* Expect = nullptr;
		if (bRan && Case->TryGetObjectField(TEXT("expect"), Expect))
		{
			for (const auto& Field : (*Expect)->Values)
			{
				FString Value;
				if (Field.Key == TEXT("return"))
				{
					const FProperty* Return = Function->GetReturnProperty();
					if (!Return)
					{
						Error = FString::Printf(TEXT("%s returns nothing"), *FunctionName);
						bOk = false;
						break;
					}
					Value = ExportField(Return, Params);
				}
				else if (const FProperty* Property = Class->FindPropertyByName(FName(Field.Key)))
				{
					Value = ExportField(Property, Target);
				}
				else if (const FProperty* Param = Function->FindPropertyByName(FName(Field.Key)))
				{
					// A parameter, as the call left it: how a by-reference argument is checked.
					Value = ExportField(Param, Params);
				}
				else
				{
					Error = FString::Printf(TEXT("%s has no field or parameter %s"), *Class->GetName(), *Field.Key);
					bOk = false;
					break;
				}
				Got->SetStringField(Field.Key, Value);
				if (Value != Field.Value->AsString())
				{
					bOk = false;
				}
			}
		}
		Function->DestroyStruct(Params);
		FMemory::Free(Params);

		// A warning from the VM means the code misbehaved, whatever the values say.
		bOk &= Messages.Lines.IsEmpty();
		Result->SetBoolField(TEXT("ok"), bOk);
		if (!Messages.Lines.IsEmpty())
		{
			TArray<TSharedPtr<FJsonValue>> Lines;
			for (const FString& Line : Messages.Lines)
			{
				Lines.Add(MakeShared<FJsonValueString>(Line));
			}
			Result->SetArrayField(TEXT("script_messages"), Lines);
		}
		if (!Error.IsEmpty())
		{
			Result->SetStringField(TEXT("error"), Error);
		}
		Result->SetObjectField(TEXT("got"), Got);
		Result->SetNumberField(TEXT("calls"), Repeat);
		Result->SetNumberField(TEXT("seconds"), Seconds);
		return Result;
	}

	/**
	 * A game world of the test's own, begun, for a Blueprint whose code needs one, such as one
	 * that sets a timer. Its actors' BeginPlay and Tick don't run, since it has no game mode, but
	 * its timers do.
	 */
	UWorld* MakeWorld()
	{
		UWorld* World = UWorld::CreateWorld(EWorldType::Game, false, TEXT("LoomTestWorld"));
		GEngine->CreateNewWorldContext(EWorldType::Game).SetCurrentWorld(World);
		World->InitializeActorsForPlay(FURL());
		World->BeginPlay();
		return World;
	}
}

int32 ULoomTestCommandlet::Main(const FString& Params)
{
	FString TestPath;
	FString ReportPath;
	if (!FParse::Value(*Params, TEXT("test="), TestPath) || !FParse::Value(*Params, TEXT("report="), ReportPath))
	{
		UE_LOG(LogLoomTest, Error, TEXT("LoomTest: usage: -run=LoomTest -test=<test.json> -report=<report.json>"));
		return 2;
	}

	TSharedRef<FJsonObject> Report = MakeShared<FJsonObject>();
	bool bOk = false;
	FString Text;
	TSharedPtr<FJsonObject> Doc;
	UObject* Target = nullptr;
	UWorld* World = nullptr;
	if (!FFileHelper::LoadFileToString(Text, *TestPath)
		|| !FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Text), Doc) || !Doc.IsValid())
	{
		Report->SetStringField(TEXT("error"), FString::Printf(TEXT("cannot read %s as JSON"), *TestPath));
	}
	else
	{
		const FString Path = Doc->GetStringField(TEXT("blueprint"));
		const UBlueprint* Blueprint = LoadObject<UBlueprint>(nullptr, *(Path + TEXT(".") + FPackageName::GetLongPackageAssetName(Path)));
		if (!Blueprint || !Blueprint->GeneratedClass)
		{
			Report->SetStringField(TEXT("error"), FString::Printf(TEXT("no compiled Blueprint at %s"), *Path));
		}
		else if (bool bWorld = false; Doc->TryGetBoolField(TEXT("world"), bWorld) && bWorld)
		{
			// Functions run on an object of the class in a world of their own, which cases tick.
			World = MakeWorld();
			UClass* Class = Blueprint->GeneratedClass;
			Target = Class->IsChildOf<AActor>() ? World->SpawnActor(Class) : NewObject<UObject>(World, Class);
			if (!Target)
			{
				Report->SetStringField(TEXT("error"), FString::Printf(TEXT("could not make a %s in the test's world"), *Class->GetName()));
			}
		}
		else
		{
			// Functions run on the class default object, so no world is needed. Cases that need
			// one, such as anything that sets a timer, ask for one with "world".
			Target = Blueprint->GeneratedClass->GetDefaultObject();
		}
	}

	if (Target)
	{
		bOk = true;
		TArray<TSharedPtr<FJsonValue>> Results;
		const TArray<TSharedPtr<FJsonValue>>* Cases = nullptr;
		if (Doc->TryGetArrayField(TEXT("cases"), Cases))
		{
			for (const TSharedPtr<FJsonValue>& Case : *Cases)
			{
				TSharedRef<FJsonObject> Result = RunCase(Target, World, Case->AsObject().ToSharedRef());
				bOk &= Result->GetBoolField(TEXT("ok"));
				UE_LOG(LogLoomTest, Display, TEXT("LoomTest: %s: %s"), *Result->GetStringField(TEXT("name")),
					Result->GetBoolField(TEXT("ok")) ? TEXT("ok") : TEXT("FAILED"));
				Results.Add(MakeShared<FJsonValueObject>(Result));
			}
		}
		Report->SetArrayField(TEXT("cases"), Results);
		Report->SetNumberField(TEXT("maximum_loop_iterations"), GEngine ? GEngine->MaximumLoopIterationCount : -1);
	}
	Report->SetBoolField(TEXT("ok"), bOk);
	if (World)
	{
		GEngine->DestroyWorldContext(World);
		World->DestroyWorld(false);
	}

	FString Out;
	FJsonSerializer::Serialize(Report, TJsonWriterFactory<>::Create(&Out));
	if (!FFileHelper::SaveStringToFile(Out, *ReportPath, FFileHelper::EEncodingOptions::ForceUTF8WithoutBOM))
	{
		UE_LOG(LogLoomTest, Error, TEXT("LoomTest: cannot write the report to %s"), *ReportPath);
		return 1;
	}
	UE_LOG(LogLoomTest, Display, TEXT("LoomTest: %s, report at %s"), bOk ? TEXT("ok") : TEXT("failed"), *ReportPath);
	return bOk ? 0 : 1;
}
