#pragma once

#include "CoreMinimal.h"
#include "Dom/JsonObject.h"

/** What the compiler type-checks against: the classes, structs and enums in memory. */
struct LOOMEDITOR_API FLoomTypes
{
	/**
	 * Loads every Blueprint under the package paths, except those in Skip. Blueprints are only in
	 * memory once loaded, and nothing loads them of its own accord, so a library's Blueprints must
	 * be loaded before a dump can see them.
	 */
	static bool LoadBlueprints(const TArray<FString>& Paths, const TSet<FName>& Skip, int32& OutLoaded, FString& OutError);

	/**
	 * Loads every Blueprint class, user-defined struct and user-defined enum, anywhere, that has
	 * one of the names (X for a Blueprint, or X_C for its generated class, which is all a cooked
	 * Blueprint has), is at one of the paths, or is under one of the folders, written with a
	 * trailing slash, except those in Skip. Every match is loaded, not
	 * just the first, so a name two assets share stays ambiguous. Returns how many were loaded
	 * that were not in memory already.
	 */
	static int32 LoadNamed(const TArray<FString>& Names, const TSet<FName>& Skip);

	/**
	 * Every loaded class's Blueprint-visible properties and callable functions, and the structs
	 * and enums they name, leaving out the classes of the packages in Skip.
	 */
	static TSharedRef<FJsonObject> Dump(const TSet<FName>& Skip);
};
