"""Makes an enum, a struct and a Blueprint the editor's way, through their factories and not by
loom, for the examples' check of assets loom didn't make. Run by the pythonscript commandlet."""

import unreal

tools = unreal.AssetToolsHelpers.get_asset_tools()
folder = "/Game/Loom/Foreign"
# A new enum has no values. A new struct has one field, MemberVar_0, stored under a generated name.
made = [
    tools.create_asset("E_Hand", folder, unreal.UserDefinedEnum, unreal.EnumFactory()),
    tools.create_asset("S_Hand", folder, unreal.UserDefinedStruct, unreal.StructureFactory()),
]
factory = unreal.BlueprintFactory()
factory.set_editor_property("parent_class", unreal.Actor)
made.append(tools.create_asset("BP_Hand", folder, unreal.Blueprint, factory))
for asset in made:
    if asset is None or not unreal.EditorAssetLibrary.save_loaded_asset(asset, False):
        raise RuntimeError(f"could not make or save {asset}")
