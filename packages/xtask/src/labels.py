"""Gives each demo's folder a Primary Asset Label, so a cook puts the folder in a pak of its own.
Run by the pythonscript commandlet, with LABELS, [[name, chunk], ...], written above it."""

import unreal

tools = unreal.AssetToolsHelpers.get_asset_tools()
for name, chunk in LABELS:  # noqa: F821, written above this script
    folder = f"/Game/Mods/{name}"
    path = f"{folder}/PAL_{name}"
    if unreal.EditorAssetLibrary.does_asset_exist(path):
        label = unreal.EditorAssetLibrary.load_asset(path)
    else:
        factory = unreal.DataAssetFactory()
        factory.set_editor_property("data_asset_class", unreal.PrimaryAssetLabel)
        label = tools.create_asset(f"PAL_{name}", folder, unreal.PrimaryAssetLabel, factory)
    rules = label.get_editor_property("rules")
    rules.set_editor_property("chunk_id", chunk)
    rules.set_editor_property("cook_rule", unreal.PrimaryAssetCookRule.ALWAYS_COOK)
    label.set_editor_property("rules", rules)
    label.set_editor_property("label_assets_in_my_directory", True)
    if not unreal.EditorAssetLibrary.save_loaded_asset(label, False):
        raise RuntimeError(f"could not save {path}")
