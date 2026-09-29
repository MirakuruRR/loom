#pragma once

#include "CoreMinimal.h"
#include "Dom/JsonObject.h"
#include "EdGraph/EdGraphPin.h"

class UBlueprint;
class UUserDefinedEnum;
class UUserDefinedStruct;
class UEdGraph;
class UEdGraphNode;
class UEdGraphPin;
class UK2Node_CreateDelegate;
class UK2Node_FunctionResult;

/**
 * Applies one ops.json document to a Blueprint, an enum or a struct. Stops at the first op that
 * fails, and records every op's outcome, the compiler's messages and a text export of each graph in
 * a JSON report.
 */
class LOOMEDITOR_API FLoomApplier
{
public:
	/**
	 * Applies Doc, which builds one Blueprint, starting with a "blueprint" op, or makes one enum or
	 * struct, with an "enum" or "struct" op, and returns whether every op succeeded and a Blueprint
	 * compiled cleanly.
	 */
	bool Apply(const TSharedRef<FJsonObject>& Doc);

	/**
	 * What the Blueprint, enum or struct is built from, saved with it as the package's "LoomBuild"
	 * metadata, so a later build can tell it is unchanged. Empty saves no record.
	 */
	FString BuildRecord;

	/** The build record a saved Blueprint, enum or struct carries, or empty when it has none. */
	static FString ReadBuildRecord(const UObject* Asset);

	/** The report of the last Apply. */
	TSharedRef<FJsonObject> GetReport() const { return Report; }

private:
	bool ApplyOp(const TSharedRef<FJsonObject>& Op, FString& OutError);

	bool OpBlueprint(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpEnum(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpStruct(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpImplement(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpVariable(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpDispatcher(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpComponent(const TSharedRef<FJsonObject>& Op, FString& OutError);
	/** Sets a member's tooltip and category from the op's "description" and "category". */
	void Describe(FName Member, const TSharedRef<FJsonObject>& Op);
	bool OpWidget(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpFunction(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpNode(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpLink(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpDefault(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpCompile(FString& OutError);
	bool OpClassDefault(const TSharedRef<FJsonObject>& Op, FString& OutError);
	bool OpSave(FString& OutError);
	static bool SaveAsset(UObject* Asset, FString& OutFile, FString& OutError);

	UK2Node_FunctionResult* MakeResultNode(UEdGraph* Graph, FString& OutError);
	/** The event graph, a function's graph, or the graph of a function of an implemented interface. */
	UEdGraph* FindGraph(const FString& Name) const;
	/** The graph made for a function of an interface the Blueprint implements. */
	UEdGraph* FindInterfaceGraph(const FString& Name) const;
	UEdGraphPin* FindPin(const FString& Ref, FString& OutError) const;
	void ExportGraphs();

	UBlueprint* Blueprint = nullptr;
	/** The enum a document that makes one made. */
	UUserDefinedEnum* Enum = nullptr;
	/** The struct a document that makes one made. */
	UUserDefinedStruct* Struct = nullptr;
	bool bCompiledClean = false;
	TMap<FString, UEdGraphNode*> Nodes;
	/** Each function's outputs, which every result node in its graph carries. */
	TMap<UEdGraph*, TArray<TPair<FName, FEdGraphPinType>>> ReturnPins;
	/** The result node made with each function that has outputs, until a "result" op takes it. */
	TMap<UEdGraph*, UK2Node_FunctionResult*> UnclaimedResults;
	/** Function graphs that override an event, whose result nodes take their outputs from it. */
	TSet<UEdGraph*> OverrideGraphs;
	/** The graphs of implemented interfaces' functions that a "function" op has filled. */
	TSet<UEdGraph*> FilledInterfaceGraphs;
	/** Each Create Event and the function it is given once its links are made. */
	TArray<TPair<UK2Node_CreateDelegate*, FName>> PendingDelegates;
	TSharedRef<FJsonObject> Report = MakeShared<FJsonObject>();
};
