using UnrealBuildTool;

public class LoomEditor : ModuleRules
{
	public LoomEditor(ReadOnlyTargetRules Target) : base(Target)
	{
		PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;
		PublicDependencyModuleNames.AddRange(new[] { "Core", "CoreUObject", "Engine" });
		PrivateDependencyModuleNames.AddRange(new[]
		{
			"UnrealEd", "BlueprintGraph", "Kismet", "KismetCompiler", "Json", "AssetRegistry", "UMG", "UMGEditor",
			"Projects", "DirectoryWatcher", "ToolMenus", "Slate", "SlateCore", "MessageLog", "ApplicationCore", "DeveloperSettings"
		});
	}
}
