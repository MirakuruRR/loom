#include "LoomLayout.h"

#include "Algo/Sort.h"
#include "EdGraph/EdGraph.h"
#include "EdGraph/EdGraphNode.h"
#include "EdGraphSchema_K2.h"
#include "K2Node.h"

namespace
{
	/** Unreal's graph grid. Every size and gap is a multiple of it, so every position lands on it. */
	constexpr int32 Grid = 16;
	constexpr int32 ColumnGap = 64;
	constexpr int32 RowGap = 48;
	/** Between one event's nodes and the next's. */
	constexpr int32 BlockGap = 128;
	/** Between a node and the pure nodes feeding it, and between those stacked above each other. */
	constexpr int32 FeedGap = 48;
	constexpr int32 StackGap = 16;
	/** How far below an exec node its feeders start, so the exec wire into it runs above them. */
	constexpr int32 ExecFeedDrop = 48;

	/** What a node's parts measure as the editor draws them, a little over rather than under. */
	constexpr float TitleCharWidth = 8.f;
	constexpr float CompactCharWidth = 16.f;
	constexpr float LabelCharWidth = 7.f;
	constexpr float TitlePadding = 64.f;
	constexpr float HeaderHeight = 32.f;
	constexpr float SubtitleHeight = 16.f;
	constexpr float PinWidth = 32.f;
	constexpr float PinHeight = 26.f;
	constexpr float BetweenSides = 32.f;
	constexpr float BottomPadding = 12.f;

	int32 SnapUp(float Value)
	{
		return FMath::CeilToInt(Value / Grid) * Grid;
	}

	bool IsShown(const UEdGraphNode* Node, const UEdGraphPin* Pin)
	{
		return !Pin->bHidden && !(Pin->bAdvancedView && Node->AdvancedPinDisplay == ENodeAdvancedPins::Hidden);
	}

	bool IsExecPin(const UEdGraphPin* Pin)
	{
		return Pin->PinType.PinCategory == UEdGraphSchema_K2::PC_Exec;
	}

	/** A node with an exec pin runs in order. One without is pure, run for whatever it feeds. */
	bool IsExecNode(const UEdGraphNode* Node)
	{
		return Node->Pins.ContainsByPredicate([Node](const UEdGraphPin* Pin) { return IsShown(Node, Pin) && IsExecPin(Pin); });
	}

	/** How wide the box for an input's value is, which the editor shows while nothing is linked to it. */
	float ValueWidth(const UEdGraphPin* Pin)
	{
		if (Pin->Direction != EGPD_Input || Pin->LinkedTo.Num() > 0 || Pin->PinType.IsContainer())
		{
			return 0.f;
		}
		const FName Category = Pin->PinType.PinCategory;
		if (Category == UEdGraphSchema_K2::PC_Boolean)
		{
			return 24.f;
		}
		if (Category == UEdGraphSchema_K2::PC_Struct)
		{
			// Wide enough for a vector's three boxes.
			return 200.f;
		}
		if (Category == UEdGraphSchema_K2::PC_Object || Category == UEdGraphSchema_K2::PC_Class
			|| Category == UEdGraphSchema_K2::PC_SoftObject || Category == UEdGraphSchema_K2::PC_SoftClass)
		{
			return Pin->PinName == UEdGraphSchema_K2::PN_Self ? 40.f : 160.f;
		}
		if (Category == UEdGraphSchema_K2::PC_Byte && Pin->PinType.PinSubCategoryObject.IsValid())
		{
			// An enum's drop-down.
			return 120.f;
		}
		static const TSet<FName> Typed = {UEdGraphSchema_K2::PC_Int, UEdGraphSchema_K2::PC_Int64, UEdGraphSchema_K2::PC_Real,
			UEdGraphSchema_K2::PC_Byte, UEdGraphSchema_K2::PC_String, UEdGraphSchema_K2::PC_Name, UEdGraphSchema_K2::PC_Text};
		if (Typed.Contains(Category))
		{
			return FMath::Clamp(Pin->GetDefaultAsString().Len() * LabelCharWidth + 24.f, 64.f, 400.f);
		}
		return 0.f;
	}

	/** The node's size as the editor will draw it: its title across the top, its pins in rows below. */
	FIntPoint EstimateSize(const UEdGraphNode* Node)
	{
		const UK2Node* K2Node = Cast<UK2Node>(Node);
		const bool bCompact = K2Node && K2Node->ShouldDrawCompact();
		float InWidth = 0.f;
		float OutWidth = 0.f;
		int32 Ins = 0;
		int32 Outs = 0;
		bool bAdvanced = false;
		for (const UEdGraphPin* Pin : Node->Pins)
		{
			bAdvanced |= Pin->bAdvancedView;
			if (!IsShown(Node, Pin))
			{
				continue;
			}
			// A compact node, such as +, shows no labels on its pins.
			const float Label = bCompact ? 0.f : Node->GetPinDisplayName(Pin).ToString().Len() * LabelCharWidth;
			const float Width = PinWidth + Label + ValueWidth(Pin);
			if (Pin->Direction == EGPD_Input)
			{
				++Ins;
				InWidth = FMath::Max(InWidth, Width);
			}
			else
			{
				++Outs;
				OutWidth = FMath::Max(OutWidth, Width);
			}
		}

		// The full title can run over several lines: its name, then such as "Target is Actor".
		TArray<FString> Lines;
		Node->GetNodeTitle(ENodeTitleType::FullTitle).ToString().ParseIntoArrayLines(Lines);
		int32 Longest = 0;
		for (const FString& Line : Lines)
		{
			Longest = FMath::Max(Longest, Line.Len());
		}
		const float TitleWidth = bCompact
			? K2Node->GetCompactNodeTitle().ToString().Len() * CompactCharWidth + TitlePadding
			: Longest * TitleCharWidth + TitlePadding;
		const float Header = bCompact ? 0.f : HeaderHeight + FMath::Max(Lines.Num() - 1, 0) * SubtitleHeight;
		const int32 PinRows = FMath::Max(Ins, Outs) + (bAdvanced ? 1 : 0);
		return FIntPoint(
			SnapUp(FMath::Max(TitleWidth, InWidth + BetweenSides + OutWidth)),
			SnapUp(Header + PinRows * PinHeight + BottomPadding));
	}

	/** Lays out one graph, one root's exec nodes at a time. */
	class FArranger
	{
	public:
		explicit FArranger(UEdGraph* InGraph) : Graph(InGraph)
		{
			for (UEdGraphNode* Node : Graph->Nodes)
			{
				Sizes.Add(Node, EstimateSize(Node));
				if (IsExecNode(Node))
				{
					ExecNodes.Add(Node);
				}
			}
		}

		void Run()
		{
			// The roots are the nodes nothing runs into: the entry, and each event. A loop nothing
			// runs into is started wherever it was made.
			TArray<UEdGraphNode*> Roots;
			for (UEdGraphNode* Node : ExecNodes)
			{
				const bool bRunInto = Node->Pins.ContainsByPredicate([](const UEdGraphPin* Pin)
					{ return Pin->Direction == EGPD_Input && IsExecPin(Pin) && Pin->LinkedTo.Num() > 0; });
				if (!bRunInto)
				{
					Roots.Add(Node);
				}
			}
			Roots.Append(ExecNodes);
			for (UEdGraphNode* Root : Roots)
			{
				if (!Visited.Contains(Root))
				{
					ArrangeBlock(Root);
				}
			}

			// Pure nodes nothing that runs uses stand in a column below the rest.
			for (UEdGraphNode* Node : Graph->Nodes)
			{
				if (!IsExecNode(Node) && !Claimed.Contains(Node))
				{
					Claimed.Add(Node);
					const FIntPoint Tree = MeasureTree(Node);
					Node->NodePosX = Tree.X - Sizes[Node].X;
					Node->NodePosY = Top;
					PlaceFeeders(Node, Node->NodePosY);
					Top += Tree.Y + BlockGap;
				}
			}
		}

	private:
		/** The exec nodes Node runs next, in the order of its outputs. */
		TArray<UEdGraphNode*> Next(const UEdGraphNode* Node) const
		{
			TArray<UEdGraphNode*> Out;
			for (const UEdGraphPin* Pin : Node->Pins)
			{
				if (Pin->Direction == EGPD_Output && IsExecPin(Pin) && IsShown(Node, Pin))
				{
					for (const UEdGraphPin* Linked : Pin->LinkedTo)
					{
						Out.AddUnique(Linked->GetOwningNode());
					}
				}
			}
			return Out;
		}

		/**
		 * Visits what runs after Node, depth first. A link to a node still being visited goes back
		 * round a loop. Node's first output carries on in its row, and each other opens a row below
		 * every row opened so far.
		 */
		void Walk(UEdGraphNode* Node, int32 InRow)
		{
			Visited.Add(Node);
			OnStack.Add(Node);
			Rows.Add(Node, InRow);
			bool bFirst = true;
			for (UEdGraphNode* Target : Next(Node))
			{
				if (OnStack.Contains(Target))
				{
					BackEdges.Add({Node, Target});
				}
				else if (!Visited.Contains(Target))
				{
					Walk(Target, bFirst ? InRow : RowCount++);
					bFirst = false;
				}
			}
			OnStack.Remove(Node);
			PostOrder.Add(Node);
		}

		/** Places the exec nodes Root reaches that no earlier root did, and what feeds them, below Top. */
		void ArrangeBlock(UEdGraphNode* Root)
		{
			PostOrder.Reset();
			RowCount = 1;
			Walk(Root, 0);
			const TSet<UEdGraphNode*> Block(PostOrder);

			// Each node's column is the longest path to it, which reverse postorder finds in one pass
			// once the links back round loops are left out.
			TMap<UEdGraphNode*, int32> Columns;
			int32 ColumnCount = 0;
			for (int32 Index = PostOrder.Num() - 1; Index >= 0; --Index)
			{
				UEdGraphNode* Node = PostOrder[Index];
				const int32 Column = Columns.FindOrAdd(Node, 0);
				ColumnCount = FMath::Max(ColumnCount, Column + 1);
				for (UEdGraphNode* Target : Next(Node))
				{
					if (Block.Contains(Target) && !BackEdges.Contains({Node, Target}))
					{
						int32& TargetColumn = Columns.FindOrAdd(Target, 0);
						TargetColumn = FMath::Max(TargetColumn, Column + 1);
					}
				}
			}

			// What feeds each node goes by the first to run, left to right and then top to bottom.
			TArray<UEdGraphNode*> Order = PostOrder;
			Algo::Sort(Order, [&](UEdGraphNode* A, UEdGraphNode* B)
			{
				return Columns[A] != Columns[B] ? Columns[A] < Columns[B] : Rows[A] < Rows[B];
			});
			// A column holds its nodes' feeders on the left and the nodes lined up on the right. A row
			// is as tall as the tallest node or stack of feeders in it.
			TArray<int32> FeedWidths, NodeWidths, Heights;
			FeedWidths.Init(0, ColumnCount);
			NodeWidths.Init(0, ColumnCount);
			Heights.Init(0, RowCount);
			for (UEdGraphNode* Node : Order)
			{
				const FIntPoint Stack = MeasureFeeders(Node);
				const int32 Column = Columns[Node];
				const int32 Row = Rows[Node];
				FeedWidths[Column] = FMath::Max(FeedWidths[Column], Stack.X > 0 ? Stack.X + FeedGap : 0);
				NodeWidths[Column] = FMath::Max(NodeWidths[Column], Sizes[Node].X);
				Heights[Row] = FMath::Max3(Heights[Row], Sizes[Node].Y, Stack.Y > 0 ? ExecFeedDrop + Stack.Y : 0);
			}
			TArray<int32> Xs, Ys;
			for (int32 Column = 0, X = 0; Column < ColumnCount; ++Column)
			{
				Xs.Add(X + FeedWidths[Column]);
				X += FeedWidths[Column] + NodeWidths[Column] + ColumnGap;
			}
			int32 Y = Top;
			for (int32 Row = 0; Row < RowCount; ++Row)
			{
				Ys.Add(Y);
				Y += Heights[Row] + RowGap;
			}
			for (UEdGraphNode* Node : Order)
			{
				Node->NodePosX = Xs[Columns[Node]];
				Node->NodePosY = Ys[Rows[Node]];
				PlaceFeeders(Node, Node->NodePosY + ExecFeedDrop);
			}
			Top = Y - RowGap + BlockGap;
		}

		/**
		 * Claims the pure nodes feeding Node that nothing has claimed, in the order of its inputs,
		 * and returns the size of them stacked, each with what feeds it to its own left.
		 */
		FIntPoint MeasureFeeders(UEdGraphNode* Node)
		{
			TArray<UEdGraphNode*> Mine;
			for (const UEdGraphPin* Pin : Node->Pins)
			{
				if (Pin->Direction != EGPD_Input || IsExecPin(Pin))
				{
					continue;
				}
				for (const UEdGraphPin* Linked : Pin->LinkedTo)
				{
					UEdGraphNode* Feeder = Linked->GetOwningNode();
					if (!IsExecNode(Feeder) && !Claimed.Contains(Feeder))
					{
						Claimed.Add(Feeder);
						Mine.Add(Feeder);
					}
				}
			}
			Feeders.Add(Node, Mine);
			FIntPoint Stack(0, 0);
			for (UEdGraphNode* Feeder : Mine)
			{
				const FIntPoint Tree = MeasureTree(Feeder);
				Stack.X = FMath::Max(Stack.X, Tree.X);
				Stack.Y += (Stack.Y > 0 ? StackGap : 0) + Tree.Y;
			}
			return Stack;
		}

		/** The size of a pure node with its feeders to its left. */
		FIntPoint MeasureTree(UEdGraphNode* Node)
		{
			const FIntPoint Stack = MeasureFeeders(Node);
			const FIntPoint Size = Sizes[Node];
			const FIntPoint Tree(Size.X + (Stack.X > 0 ? FeedGap + Stack.X : 0), FMath::Max(Size.Y, Stack.Y));
			Trees.Add(Node, Tree);
			return Tree;
		}

		/** Stacks Node's feeders down from From, each with its right edge a gap left of Node. */
		void PlaceFeeders(const UEdGraphNode* Node, int32 From)
		{
			const int32 Right = Node->NodePosX - FeedGap;
			int32 Y = From;
			for (UEdGraphNode* Feeder : Feeders.FindRef(Node))
			{
				Feeder->NodePosX = Right - Sizes[Feeder].X;
				Feeder->NodePosY = Y;
				PlaceFeeders(Feeder, Y);
				Y += Trees[Feeder].Y + StackGap;
			}
		}

		UEdGraph* Graph;
		TMap<UEdGraphNode*, FIntPoint> Sizes;
		TArray<UEdGraphNode*> ExecNodes;
		/** Where the next block starts. */
		int32 Top = 0;

		TSet<UEdGraphNode*> Visited;
		TSet<UEdGraphNode*> OnStack;
		TSet<TPair<UEdGraphNode*, UEdGraphNode*>> BackEdges;
		TArray<UEdGraphNode*> PostOrder;
		TMap<UEdGraphNode*, int32> Rows;
		int32 RowCount = 0;

		/** Pure nodes that have their place, beside the first node found that they feed. */
		TSet<UEdGraphNode*> Claimed;
		TMap<UEdGraphNode*, TArray<UEdGraphNode*>> Feeders;
		TMap<UEdGraphNode*, FIntPoint> Trees;
	};

	FString NameOf(const UEdGraphNode* Node)
	{
		return FString::Printf(TEXT("%s (%s)"), *Node->GetNodeTitle(ENodeTitleType::ListView).ToString(), *Node->GetName());
	}
}

void FLoomLayout::Arrange(UEdGraph* Graph)
{
	FArranger(Graph).Run();
}

bool FLoomLayout::CheckNoOverlap(const UEdGraph* Graph, FString& OutError)
{
	TArray<TPair<const UEdGraphNode*, FIntRect>> Boxes;
	for (const UEdGraphNode* Node : Graph->Nodes)
	{
		const FIntPoint Min(Node->NodePosX, Node->NodePosY);
		const FIntRect Box(Min, Min + EstimateSize(Node));
		for (const TPair<const UEdGraphNode*, FIntRect>& Other : Boxes)
		{
			if (Box.Intersect(Other.Value))
			{
				OutError = FString::Printf(TEXT("in %s, %s at %d,%d overlaps %s at %d,%d"), *Graph->GetName(), *NameOf(Node),
					Box.Min.X, Box.Min.Y, *NameOf(Other.Key), Other.Value.Min.X, Other.Value.Min.Y);
				return false;
			}
		}
		Boxes.Emplace(Node, Box);
	}
	return true;
}
