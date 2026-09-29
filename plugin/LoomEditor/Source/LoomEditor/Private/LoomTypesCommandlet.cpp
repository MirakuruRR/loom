#include "LoomTypesCommandlet.h"

#include "LoomTypes.h"
#include "Misc/FileHelper.h"
#include "Serialization/JsonSerializer.h"

DEFINE_LOG_CATEGORY_STATIC(LogLoomTypes, Log, All);

int32 ULoomTypesCommandlet::Main(const FString& Params)
{
	FString OutPath;
	if (!FParse::Value(*Params, TEXT("report="), OutPath))
	{
		UE_LOG(LogLoomTypes, Error, TEXT("LoomTypes: usage: -run=LoomTypes -report=<types.json>"));
		return 2;
	}

	// The Blueprints under each -load= path, such as a library's, are loaded before the dump.
	FString LoadPaths;
	if (FParse::Value(*Params, TEXT("load="), LoadPaths))
	{
		TArray<FString> Paths;
		LoadPaths.ParseIntoArray(Paths, TEXT(","));
		int32 Loaded = 0;
		FString Error;
		if (!FLoomTypes::LoadBlueprints(Paths, {}, Loaded, Error))
		{
			UE_LOG(LogLoomTypes, Error, TEXT("LoomTypes: %s"), *Error);
			return 1;
		}
		UE_LOG(LogLoomTypes, Display, TEXT("LoomTypes: loaded %d Blueprints under %s"), Loaded, *LoadPaths);
	}

	const TSharedRef<FJsonObject> Report = FLoomTypes::Dump({});
	FString Text;
	FJsonSerializer::Serialize(Report, TJsonWriterFactory<TCHAR, TCondensedJsonPrintPolicy<TCHAR>>::Create(&Text));
	if (!FFileHelper::SaveStringToFile(Text, *OutPath, FFileHelper::EEncodingOptions::ForceUTF8WithoutBOM))
	{
		UE_LOG(LogLoomTypes, Error, TEXT("LoomTypes: cannot write %s"), *OutPath);
		return 1;
	}
	UE_LOG(LogLoomTypes, Display, TEXT("LoomTypes: ok, %d classes, %d structs, %d enums, report at %s"),
		Report->GetObjectField(TEXT("classes"))->Values.Num(), Report->GetObjectField(TEXT("structs"))->Values.Num(),
		Report->GetObjectField(TEXT("enums"))->Values.Num(), *OutPath);
	return 0;
}
