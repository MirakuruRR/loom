#pragma once

#include "CoreMinimal.h"

class UEdGraph;

/**
 * Places the nodes of a graph loom has built, since ops.json says nothing of where they go. Exec
 * nodes stand in columns by the longest exec path to them from the graph's entry or event, and in
 * rows that split where a node has several exec outputs, so an if/else opens into two rows. Pure
 * nodes stand to the left of the node they feed. A node's size is only known once Slate draws it,
 * so each is estimated from its title and pins, and the layout leaves each the room its estimate
 * needs.
 */
struct FLoomLayout
{
	/** Places every node in Graph. In an event graph, each event's nodes go below the last's. */
	static void Arrange(UEdGraph* Graph);

	/** Whether no two nodes' estimated rectangles in Graph overlap. If two do, OutError names them. */
	static bool CheckNoOverlap(const UEdGraph* Graph, FString& OutError);
};
