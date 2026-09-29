#include "LoomBuildCommandlet.h"

#include "LoomBuild.h"
#include "Misc/FileHelper.h"

DEFINE_LOG_CATEGORY_STATIC(LogLoomBuildCommandlet, Log, All);

int32 ULoomBuildCommandlet::Main(const FString& Params)
{
	const TSharedRef<FJsonObject> Report = FLoomBuild::Run(FParse::Param(*Params, TEXT("force")));
	for (const TSharedPtr<FJsonValue>& Value : Report->GetArrayField(TEXT("errors")))
	{
		const TSharedPtr<FJsonObject>& Error = Value->AsObject();
		FString File = Error->GetStringField(TEXT("file"));
		int32 Line = 0;
		if (Error->TryGetNumberField(TEXT("line"), Line))
		{
			File += FString::Printf(TEXT(":%d:%d"), Line, (int32)Error->GetNumberField(TEXT("column")));
		}
		UE_LOG(LogLoomBuildCommandlet, Error, TEXT("LoomBuild: %s%s%s"), *File, File.IsEmpty() ? TEXT("") : TEXT(": "),
			*Error->GetStringField(TEXT("message")));
	}
	FString ReportPath;
	if (FParse::Value(*Params, TEXT("report="), ReportPath)
		&& !FFileHelper::SaveStringToFile(FLoomBuild::ToText(Report), *ReportPath, FFileHelper::EEncodingOptions::ForceUTF8WithoutBOM))
	{
		UE_LOG(LogLoomBuildCommandlet, Error, TEXT("LoomBuild: cannot write %s"), *ReportPath);
		return 1;
	}
	return Report->GetBoolField(TEXT("ok")) ? 0 : 1;
}
