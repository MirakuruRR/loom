#include "LoomLibrary.h"

#include "LoomBuild.h"
#include "LoomEditorModule.h"

FString ULoomLibrary::BuildBlueprints(bool bForce)
{
	return FLoomBuild::ToText(FLoomEditorModule::BuildAndShow(bForce));
}
