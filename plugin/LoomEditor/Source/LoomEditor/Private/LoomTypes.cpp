#include "LoomTypes.h"

#include "AssetRegistry/AssetRegistryModule.h"
#include "Components/PanelWidget.h"
#include "EdGraphSchema_K2.h"
#include "Engine/Blueprint.h"
#include "Engine/BlueprintGeneratedClass.h"
#include "Engine/UserDefinedEnum.h"
#include "Kismet2/KismetEditorUtilities.h"
#include "LoomSettings.h"
#include "Misc/PackageName.h"
#include "Serialization/JsonSerializer.h"
#include "StructUtils/UserDefinedStruct.h"
#include "UObject/EnumProperty.h"
#include "UObject/TextProperty.h"
#include "UObject/UObjectGlobals.h"
#include "UObject/UObjectIterator.h"

namespace
{
	/**
	 * A property's type in Loom's spelling: bool, int, float, object:<class path>,
	 * array<struct:<path>> and so on. Structs and enums it names are added to the sets, so their
	 * own members can be dumped after the classes.
	 */
	FString TypeOf(const FProperty* Property, TSet<const UScriptStruct*>& Structs, TSet<const UEnum*>& Enums)
	{
		if (CastField<FBoolProperty>(Property)) return TEXT("bool");
		if (const FByteProperty* Byte = CastField<FByteProperty>(Property))
		{
			if (Byte->Enum)
			{
				Enums.Add(Byte->Enum);
				return TEXT("enum:") + Byte->Enum->GetPathName();
			}
			return TEXT("byte");
		}
		if (const FEnumProperty* Enum = CastField<FEnumProperty>(Property))
		{
			Enums.Add(Enum->GetEnum());
			return TEXT("enum:") + GetPathNameSafe(Enum->GetEnum());
		}
		if (CastField<FIntProperty>(Property)) return TEXT("int");
		if (CastField<FInt64Property>(Property)) return TEXT("int64");
		if (CastField<FFloatProperty>(Property)) return TEXT("float32");
		if (CastField<FDoubleProperty>(Property)) return TEXT("float");
		if (CastField<FNameProperty>(Property)) return TEXT("name");
		if (CastField<FStrProperty>(Property)) return TEXT("string");
		if (CastField<FTextProperty>(Property)) return TEXT("text");
		// Class before object, and the soft and weak kinds before either, since each derives from the next.
		if (const FClassProperty* Class = CastField<FClassProperty>(Property)) return TEXT("class:") + GetPathNameSafe(Class->MetaClass);
		if (const FSoftClassProperty* SoftClass = CastField<FSoftClassProperty>(Property)) return TEXT("softclass:") + GetPathNameSafe(SoftClass->MetaClass);
		if (const FSoftObjectProperty* Soft = CastField<FSoftObjectProperty>(Property)) return TEXT("softobject:") + GetPathNameSafe(Soft->PropertyClass);
		if (const FWeakObjectProperty* Weak = CastField<FWeakObjectProperty>(Property)) return TEXT("weak:") + GetPathNameSafe(Weak->PropertyClass);
		if (const FObjectPropertyBase* Object = CastField<FObjectPropertyBase>(Property)) return TEXT("object:") + GetPathNameSafe(Object->PropertyClass);
		if (const FInterfaceProperty* Interface = CastField<FInterfaceProperty>(Property)) return TEXT("interface:") + GetPathNameSafe(Interface->InterfaceClass);
		if (const FStructProperty* Struct = CastField<FStructProperty>(Property))
		{
			Structs.Add(Struct->Struct);
			return TEXT("struct:") + GetPathNameSafe(Struct->Struct);
		}
		if (const FArrayProperty* Array = CastField<FArrayProperty>(Property))
		{
			return FString::Printf(TEXT("array<%s>"), *TypeOf(Array->Inner, Structs, Enums));
		}
		if (const FSetProperty* Set = CastField<FSetProperty>(Property))
		{
			return FString::Printf(TEXT("set<%s>"), *TypeOf(Set->ElementProp, Structs, Enums));
		}
		if (const FMapProperty* Map = CastField<FMapProperty>(Property))
		{
			return FString::Printf(TEXT("map<%s,%s>"), *TypeOf(Map->KeyProp, Structs, Enums), *TypeOf(Map->ValueProp, Structs, Enums));
		}
		// A delegate is named by its signature, which says what a function bound to it takes.
		if (const FDelegateProperty* Delegate = CastField<FDelegateProperty>(Property)) return TEXT("delegate:") + GetPathNameSafe(Delegate->SignatureFunction);
		if (CastField<FMulticastDelegateProperty>(Property)) return TEXT("multicast_delegate");
		return TEXT("unknown:") + Property->GetClass()->GetName();
	}

	/**
	 * How a parameter is passed, as its "dir": in, ref, out or return. A const reference is an
	 * input like any other, and only a mutable one works on the caller's variable, but a function
	 * bound to a delegate must take a const reference where the delegate passes one, so it is
	 * marked "const_ref" too.
	 */
	const TCHAR* SetDirection(FJsonObject& Param, const FProperty* Property)
	{
		const bool bConstRef = Property->HasAllPropertyFlags(CPF_ReferenceParm | CPF_ConstParm) && !Property->HasAnyPropertyFlags(CPF_ReturnParm);
		const TCHAR* Direction = Property->HasAnyPropertyFlags(CPF_ReturnParm) ? TEXT("return")
			: bConstRef ? TEXT("in")
			: Property->HasAnyPropertyFlags(CPF_ReferenceParm) ? TEXT("ref")
			: Property->HasAnyPropertyFlags(CPF_OutParm) ? TEXT("out")
			: TEXT("in");
		Param.SetStringField(TEXT("dir"), Direction);
		if (bConstRef)
		{
			Param.SetBoolField(TEXT("const_ref"), true);
		}
		return Direction;
	}

	TSharedRef<FJsonObject> DumpProperty(const FProperty* Property, TSet<const UScriptStruct*>& Structs, TSet<const UEnum*>& Enums)
	{
		TSharedRef<FJsonObject> Out = MakeShared<FJsonObject>();
		Out->SetStringField(TEXT("type"), TypeOf(Property, Structs, Enums));
		if (Property->HasAnyPropertyFlags(CPF_BlueprintReadOnly))
		{
			Out->SetBoolField(TEXT("read_only"), true);
		}
		// A dispatcher's or a delegate's signature: what a function bound to it is passed, and how.
		// Blueprint binds a function only if each of its parameters is passed the same way: by
		// value, by reference, const or not, as an output, or as the return value.
		const UFunction* Signature = nullptr;
		if (const FMulticastDelegateProperty* Dispatcher = CastField<FMulticastDelegateProperty>(Property))
		{
			Signature = Dispatcher->SignatureFunction;
		}
		else if (const FDelegateProperty* Delegate = CastField<FDelegateProperty>(Property))
		{
			Signature = Delegate->SignatureFunction;
		}
		if (Signature)
		{
			TArray<TSharedPtr<FJsonValue>> Params;
			for (TFieldIterator<FProperty> It(Signature); It && It->HasAnyPropertyFlags(CPF_Parm); ++It)
			{
				TSharedRef<FJsonObject> Param = MakeShared<FJsonObject>();
				Param->SetStringField(TEXT("name"), It->GetName());
				Param->SetStringField(TEXT("type"), TypeOf(*It, Structs, Enums));
				SetDirection(*Param, *It);
				Params.Add(MakeShared<FJsonValueObject>(Param));
			}
			Out->SetArrayField(TEXT("signature"), Params);
		}
		return Out;
	}

	TSharedRef<FJsonObject> DumpFunction(const UFunction* Function, TSet<const UScriptStruct*>& Structs, TSet<const UEnum*>& Enums)
	{
		TSharedRef<FJsonObject> Out = MakeShared<FJsonObject>();
		TArray<TSharedPtr<FJsonValue>> Flags;
		auto Flag = [&Flags](bool bSet, const TCHAR* Name)
		{
			if (bSet)
			{
				Flags.Add(MakeShared<FJsonValueString>(Name));
			}
		};
		Flag(Function->HasAnyFunctionFlags(FUNC_BlueprintPure), TEXT("pure"));
		Flag(Function->HasAnyFunctionFlags(FUNC_Static), TEXT("static"));
		Flag(Function->HasAnyFunctionFlags(FUNC_Const), TEXT("const"));
		Flag(Function->HasAnyFunctionFlags(FUNC_BlueprintEvent), TEXT("event"));
		Flag(!Function->HasAnyFunctionFlags(FUNC_BlueprintCallable | FUNC_BlueprintPure), TEXT("not_callable"));
		Flag(Function->HasMetaData(TEXT("Latent")), TEXT("latent"));
		Flag(Function->HasMetaData(TEXT("BlueprintInternalUseOnly")), TEXT("internal"));
		Flag(Function->HasMetaData(TEXT("DeprecatedFunction")), TEXT("deprecated"));
		Flag(Function->HasMetaData(TEXT("WorldContext")), TEXT("world_context"));
		// Callable only from editor utilities, not from a Blueprint that runs in the game.
		Flag(Function->HasAnyFunctionFlags(FUNC_EditorOnly), TEXT("editor_only"));
		// Who besides the class that declares it may call it.
		Flag(Function->HasAnyFunctionFlags(FUNC_Private), TEXT("private"));
		Flag(Function->HasAnyFunctionFlags(FUNC_Protected), TEXT("protected"));
		Out->SetArrayField(TEXT("flags"), Flags);
		// Parameters the call node fills or hides by itself, and the one whose class decides the
		// type of the return value.
		for (const TCHAR* Key : { TEXT("WorldContext"), TEXT("DefaultToSelf"), TEXT("HidePin"), TEXT("DeterminesOutputType") })
		{
			if (Function->HasMetaData(Key))
			{
				FString Field = FString(Key).Replace(TEXT("WorldContext"), TEXT("world_context"))
					.Replace(TEXT("DefaultToSelf"), TEXT("default_to_self")).Replace(TEXT("HidePin"), TEXT("hide_pin"))
					.Replace(TEXT("DeterminesOutputType"), TEXT("determines_output_type"));
				Out->SetStringField(Field, Function->GetMetaData(Key));
			}
		}

		// For a function of an array, a set or a map, what each parameter's type is tied to, as
		// the metadata that makes its pins wildcards says: the container, what it holds, or a
		// map's keys or values. The dump writes such a type with int for what it holds.
		TMap<FString, FString> Roles;
		auto Mark = [&Roles](const FString& Names, const TCHAR* Role)
		{
			TArray<FString> Split;
			Names.ParseIntoArray(Split, TEXT(","), true);
			for (FString& Name : Split)
			{
				Name.TrimStartAndEndInline();
				if (!Name.IsEmpty() && !Roles.Contains(Name))
				{
					Roles.Add(Name, Role);
				}
			}
		};
		Mark(Function->GetMetaData(TEXT("ArrayParm")), TEXT("container"));
		Mark(Function->GetMetaData(TEXT("ArrayTypeDependentParams")), TEXT("item"));
		// A set's groups are split by commas, and the pins in a group, split by |, share what they
		// hold; which of them is a set is its own type's to say.
		Mark(Function->GetMetaData(TEXT("SetParam")).Replace(TEXT("|"), TEXT(",")), TEXT("set"));
		Mark(Function->GetMetaData(TEXT("MapParam")), TEXT("container"));
		Mark(Function->GetMetaData(TEXT("MapKeyParam")), TEXT("key"));
		Mark(Function->GetMetaData(TEXT("MapValueParam")), TEXT("value"));

		TArray<FString> AutoTerms;
		UEdGraphSchema_K2::GetAutoEmitTermParameters(Function, AutoTerms);
		TArray<TSharedPtr<FJsonValue>> Params;
		for (TFieldIterator<FProperty> It(Function); It && It->HasAnyPropertyFlags(CPF_Parm); ++It)
		{
			TSharedRef<FJsonObject> Param = DumpProperty(*It, Structs, Enums);
			Param->SetStringField(TEXT("name"), It->GetName());
			const TCHAR* Direction = SetDirection(*Param, *It);
			// Some const references still make a by-reference pin, which the editor will not
			// let hold a value written on it, unless the function asks for one to be made.
			if (FCString::Strcmp(Direction, TEXT("in")) == 0
				&& It->HasAllPropertyFlags(CPF_OutParm | CPF_ReferenceParm)
				&& !AutoTerms.Contains(It->GetName()))
			{
				Param->SetBoolField(TEXT("wired"), true);
			}
			Param->RemoveField(TEXT("read_only"));
			if (const FString* Role = Roles.Find(It->GetName()))
			{
				Param->SetStringField(TEXT("generic"), *Role == TEXT("set") ? (CastField<FSetProperty>(*It) ? TEXT("container") : TEXT("item")) : **Role);
			}
			const FString DefaultKey = TEXT("CPP_Default_") + It->GetName();
			if (Function->HasMetaData(*DefaultKey))
			{
				Param->SetStringField(TEXT("default"), Function->GetMetaData(*DefaultKey));
			}
			Params.Add(MakeShared<FJsonValueObject>(Param));
		}
		Out->SetArrayField(TEXT("params"), Params);
		return Out;
	}

	/**
	 * A property Blueprints can see, or one the Details panel edits, which a component's or a
	 * class's defaults may set though code cannot reach it. A fixed-size array is left out: its
	 * elements have no one name to set.
	 */
	bool IsDumpable(const FProperty* Property)
	{
		const bool bVisible = Property->HasAnyPropertyFlags(CPF_BlueprintVisible | CPF_BlueprintAssignable);
		const bool bEditable = Property->HasAnyPropertyFlags(CPF_Edit)
			&& !Property->HasAnyPropertyFlags(CPF_EditConst | CPF_Deprecated) && Property->ArrayDim == 1;
		return bVisible || bEditable;
	}

	/** A property's record, as a member of a class or a struct. */
	TSharedRef<FJsonObject> DumpMember(const FProperty* Property, TSet<const UScriptStruct*>& Structs, TSet<const UEnum*>& Enums)
	{
		TSharedRef<FJsonObject> Out = DumpProperty(Property, Structs, Enums);
		if (!Property->HasAnyPropertyFlags(CPF_BlueprintVisible | CPF_BlueprintAssignable))
		{
			Out->SetBoolField(TEXT("edit_only"), true);
		}
		// A Blueprint variable marked Private, which only its own class can read or set.
		if (Property->GetBoolMetaData(FBlueprintMetadata::MD_Private))
		{
			Out->SetBoolField(TEXT("private"), true);
		}
		return Out;
	}

	bool IsDumpable(const UClass* Class)
	{
		const FString Name = Class->GetName();
		return !Class->HasAnyClassFlags(CLASS_NewerVersionExists | CLASS_Deprecated)
			&& !Name.StartsWith(TEXT("SKEL_")) && !Name.StartsWith(TEXT("REINST_")) && !Name.StartsWith(TEXT("TRASHCLASS_"))
			&& !Name.StartsWith(TEXT("PLACEHOLDER-")) && Class->GetOutermost() != GetTransientPackage();
	}
}

bool FLoomTypes::LoadBlueprints(const TArray<FString>& Paths, const TSet<FName>& Skip, int32& OutLoaded, FString& OutError)
{
	OutLoaded = 0;
	if (Paths.IsEmpty())
	{
		return true;
	}
	IAssetRegistry& Registry = FAssetRegistryModule::GetRegistry();
	Registry.SearchAllAssets(true);
	FARFilter Filter;
	// A cooked Blueprint has only its generated class.
	Filter.ClassPaths.Add(UBlueprint::StaticClass()->GetClassPathName());
	Filter.ClassPaths.Add(UBlueprintGeneratedClass::StaticClass()->GetClassPathName());
	Filter.bRecursiveClasses = true;
	Filter.bRecursivePaths = true;
	for (const FString& Path : Paths)
	{
		Filter.PackagePaths.Add(FName(Path));
	}
	TArray<FAssetData> Found;
	Registry.GetAssets(Filter, Found);
	for (const FAssetData& Asset : Found)
	{
		if (Skip.Contains(Asset.PackageName))
		{
			continue;
		}
		++OutLoaded;
		const UObject* Object = Asset.GetAsset();
		const UBlueprint* Blueprint = Cast<UBlueprint>(Object);
		if (!Object || (Blueprint && !Blueprint->GeneratedClass))
		{
			OutError = FString::Printf(TEXT("could not load %s"), *Asset.GetObjectPathString());
			return false;
		}
	}
	return true;
}

int32 FLoomTypes::LoadNamed(const TArray<FString>& Names, const TSet<FName>& Skip)
{
	IAssetRegistry& Registry = FAssetRegistryModule::GetRegistry();
	Registry.SearchAllAssets(true);
	FARFilter Filter;
	Filter.ClassPaths.Add(UBlueprint::StaticClass()->GetClassPathName());
	Filter.ClassPaths.Add(UBlueprintGeneratedClass::StaticClass()->GetClassPathName());
	Filter.ClassPaths.Add(UUserDefinedStruct::StaticClass()->GetClassPathName());
	Filter.ClassPaths.Add(UUserDefinedEnum::StaticClass()->GetClassPathName());
	Filter.bRecursiveClasses = true;

	TSet<FName> Wanted;
	FARFilter ByPath = Filter;
	FARFilter ByFolder = Filter;
	ByFolder.bRecursivePaths = true;
	for (const FString& Name : Names)
	{
		if (Name.EndsWith(TEXT("/")))
		{
			// A folder a source uses: everything under it.
			ByFolder.PackagePaths.Add(FName(Name.LeftChop(1)));
		}
		else if (Name.StartsWith(TEXT("/")))
		{
			ByPath.PackageNames.Add(FName(FPackageName::ObjectPathToPackageName(Name)));
		}
		else
		{
			Wanted.Add(FName(Name));
			Wanted.Add(FName(Name + TEXT("_C")));
		}
	}
	TArray<FAssetData> Found;
	if (!Wanted.IsEmpty())
	{
		TArray<FAssetData> All;
		Registry.GetAssets(Filter, All);
		Found.Append(All.FilterByPredicate([&Wanted](const FAssetData& Asset) { return Wanted.Contains(Asset.AssetName); }));
	}
	if (!ByPath.PackageNames.IsEmpty())
	{
		Registry.GetAssets(ByPath, Found);
	}
	if (!ByFolder.PackagePaths.IsEmpty())
	{
		Registry.GetAssets(ByFolder, Found);
	}
	int32 Loaded = 0;
	for (const FAssetData& Asset : Found)
	{
		if (!Skip.Contains(Asset.PackageName) && !Asset.IsAssetLoaded() && Asset.GetAsset())
		{
			++Loaded;
		}
	}
	return Loaded;
}

TSharedRef<FJsonObject> FLoomTypes::Dump(const TSet<FName>& Skip)
{
	TSet<const UScriptStruct*> Structs;
	TSet<const UEnum*> Enums;
	TSharedRef<FJsonObject> Classes = MakeShared<FJsonObject>();
	for (TObjectIterator<UClass> It; It; ++It)
	{
		const UClass* Class = *It;
		if (!IsDumpable(Class) || Skip.Contains(Class->GetOutermost()->GetFName()))
		{
			continue;
		}
		TSharedRef<FJsonObject> Out = MakeShared<FJsonObject>();
		if (Class->GetSuperClass())
		{
			Out->SetStringField(TEXT("super"), Class->GetSuperClass()->GetPathName());
		}
		if (Class->HasAnyClassFlags(CLASS_Abstract))
		{
			Out->SetBoolField(TEXT("abstract"), true);
		}
		// Editor modules are not in a packaged game, so a Blueprint must not call into them.
		if (IsEditorOnlyObject(Class))
		{
			Out->SetBoolField(TEXT("editor_only"), true);
		}
		if (Class->HasAnyClassFlags(CLASS_Interface))
		{
			Out->SetBoolField(TEXT("interface"), true);
			if (!FKismetEditorUtilities::IsClassABlueprintImplementableInterface(Class))
			{
				Out->SetBoolField(TEXT("not_implementable"), true);
			}
		}
		// The interfaces the class implements itself, as a Blueprint's Class Settings list them. Its
		// parents' are in their own records.
		TArray<TSharedPtr<FJsonValue>> Interfaces;
		for (const FImplementedInterface& Implemented : Class->Interfaces)
		{
			if (Implemented.Class)
			{
				Interfaces.Add(MakeShared<FJsonValueString>(Implemented.Class->GetPathName()));
			}
		}
		if (!Interfaces.IsEmpty())
		{
			Out->SetArrayField(TEXT("interfaces"), Interfaces);
		}
		// A panel's children are each held by a slot of its own class, which a tree sets fields of.
		// Only the class that changes either is written; its subclasses inherit them.
		if (Class->IsChildOf<UPanelWidget>())
		{
			const UPanelWidget* Panel = Class->GetDefaultObject<UPanelWidget>();
			const UPanelWidget* Super = Class->GetSuperClass()->IsChildOf<UPanelWidget>()
				? Class->GetSuperClass()->GetDefaultObject<UPanelWidget>() : nullptr;
			if (!Super || Super->GetSlotClass() != Panel->GetSlotClass() || Super->CanHaveMultipleChildren() != Panel->CanHaveMultipleChildren())
			{
				Out->SetStringField(TEXT("slot"), Panel->GetSlotClass()->GetPathName());
				if (!Panel->CanHaveMultipleChildren())
				{
					Out->SetBoolField(TEXT("one_child"), true);
				}
			}
		}
		TSharedRef<FJsonObject> Properties = MakeShared<FJsonObject>();
		for (TFieldIterator<FProperty> Prop(Class, EFieldIteratorFlags::ExcludeSuper); Prop; ++Prop)
		{
			if (IsDumpable(*Prop))
			{
				Properties->SetObjectField(Prop->GetName(), DumpMember(*Prop, Structs, Enums));
			}
		}
		TSharedRef<FJsonObject> Functions = MakeShared<FJsonObject>();
		for (TFieldIterator<UFunction> Fn(Class, EFieldIteratorFlags::ExcludeSuper); Fn; ++Fn)
		{
			if (Fn->HasAnyFunctionFlags(FUNC_BlueprintCallable | FUNC_BlueprintPure | FUNC_BlueprintEvent))
			{
				Functions->SetObjectField(Fn->GetName(), DumpFunction(*Fn, Structs, Enums));
			}
		}
		Out->SetObjectField(TEXT("properties"), Properties);
		Out->SetObjectField(TEXT("functions"), Functions);
		Classes->SetObjectField(Class->GetPathName(), Out);
	}

	// A struct or enum made in the editor is named by no class until a source uses it, so every one
	// that is loaded is written too.
	const auto IsUserMade = [&Skip](const UObject* Object)
	{
		const FString Name = Object->GetName();
		return IsValid(Object) && Object->GetOutermost() != GetTransientPackage()
			&& !Skip.Contains(Object->GetOutermost()->GetFName())
			&& !Name.StartsWith(TEXT("TRASH")) && !Name.Contains(TEXT("REINST"));
	};
	for (TObjectIterator<UUserDefinedStruct> It; It; ++It)
	{
		if (IsUserMade(*It))
		{
			Structs.Add(*It);
		}
	}
	for (TObjectIterator<UUserDefinedEnum> It; It; ++It)
	{
		if (IsUserMade(*It))
		{
			Enums.Add(*It);
		}
	}

	// Structs can name further structs, so the set grows while it is walked.
	TSharedRef<FJsonObject> StructsOut = MakeShared<FJsonObject>();
	TArray<const UScriptStruct*> Pending = Structs.Array();
	TSet<const UScriptStruct*> Done;
	while (Pending.Num() > 0)
	{
		const UScriptStruct* Struct = Pending.Pop();
		if (!Struct || Done.Contains(Struct))
		{
			continue;
		}
		Done.Add(Struct);
		TSet<const UScriptStruct*> Found;
		TSharedRef<FJsonObject> Fields = MakeShared<FJsonObject>();
		for (TFieldIterator<FProperty> Prop(Struct); Prop; ++Prop)
		{
			if (IsDumpable(*Prop))
			{
				// A user-defined struct's field is named Name_2_<guid>, and called by the name its
				// author gave it, which text such as a default's uses too.
				Fields->SetObjectField(Struct->GetAuthoredNameForField(*Prop), DumpMember(*Prop, Found, Enums));
			}
		}
		Pending.Append(Found.Array());
		TSharedRef<FJsonObject> Out = MakeShared<FJsonObject>();
		Out->SetObjectField(TEXT("fields"), Fields);
		// Structs like Vector2D refuse the generic make and break nodes and name functions instead.
		if (Struct->HasMetaData(TEXT("HasNativeMake")))
		{
			Out->SetStringField(TEXT("native_make"), Struct->GetMetaData(TEXT("HasNativeMake")));
		}
		if (Struct->HasMetaData(TEXT("HasNativeBreak")))
		{
			Out->SetStringField(TEXT("native_break"), Struct->GetMetaData(TEXT("HasNativeBreak")));
		}
		StructsOut->SetObjectField(Struct->GetPathName(), Out);
	}

	TSharedRef<FJsonObject> EnumsOut = MakeShared<FJsonObject>();
	for (const UEnum* Enum : Enums)
	{
		if (!Enum)
		{
			continue;
		}
		TArray<TSharedPtr<FJsonValue>> Names;
		// The last entry is the generated _MAX.
		for (int32 Index = 0; Index < Enum->NumEnums() - 1; ++Index)
		{
			Names.Add(MakeShared<FJsonValueString>(Enum->GetNameStringByIndex(Index)));
		}
		EnumsOut->SetArrayField(Enum->GetPathName(), Names);
	}

	TSharedRef<FJsonObject> Report = MakeShared<FJsonObject>();
	Report->SetObjectField(TEXT("classes"), Classes);
	Report->SetObjectField(TEXT("structs"), StructsOut);
	Report->SetObjectField(TEXT("enums"), EnumsOut);

	// The settings the compiler checks against, read here so Unreal's config is the one reader.
	TSharedRef<FJsonObject> Settings = MakeShared<FJsonObject>();
	TArray<TSharedPtr<FJsonValue>> Paths;
	for (const FString& Path : GetDefault<ULoomSettings>()->InstancedClassPaths)
	{
		Paths.Add(MakeShared<FJsonValueString>(Path));
	}
	Settings->SetArrayField(TEXT("instanced_class_paths"), Paths);
	Report->SetObjectField(TEXT("settings"), Settings);
	return Report;
}
