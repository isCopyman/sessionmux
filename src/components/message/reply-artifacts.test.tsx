import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, fireEvent } from "@testing-library/react"
import { ReplyArtifacts } from "./reply-artifacts"
import type { FileChangeStat } from "@/lib/session-files"
import type { MessageTurn } from "@/lib/types"

// Stable `t` (per next-intl mock guidance) returns the key verbatim — enough to
// address every label here (section headers, the per-file `viewDiff` /
// `revealInFolder` actions, the `noDiffDataAvailable` fallback).
const { stableT, mockOpenDiff, mockOpenFilePreview, mockReveal, mockExtract } =
  vi.hoisted(() => ({
    stableT: (key: string) => key,
    mockOpenDiff: vi.fn(),
    mockOpenFilePreview: vi.fn(),
    mockReveal: vi.fn(),
    mockExtract: vi.fn(),
  }))

vi.mock("next-intl", () => ({ useTranslations: () => stableT }))
vi.mock("@/contexts/workspace-context", () => ({
  useWorkspaceActions: () => ({
    openFilePreview: mockOpenFilePreview,
    openSessionFileDiff: mockOpenDiff,
  }),
}))
vi.mock("@/contexts/active-folder-context", () => ({
  useActiveFolder: () => ({ activeFolder: { path: "/repo" } }),
}))
vi.mock("@/lib/platform", () => ({
  isLocalDesktop: () => true,
  revealItemInDir: mockReveal,
  // Pulled in (unused here) by the real `UnifiedDiffPreview` import chain; a
  // factory mock replaces the whole module, so the export must exist.
  openUrl: vi.fn(),
}))
// Drive the card's file list directly — the extractor itself is covered by
// session-files' own tests; here we only wire the parsed shape into the UI.
vi.mock("@/lib/session-files", () => ({
  extractReplyFileChanges: (turns: unknown) => mockExtract(turns),
}))

const MODIFIED_DIFF =
  "diff --git a/src/a.ts b/src/a.ts\n@@ -1,2 +1,2 @@\n-old\n+new"
const DELETION_DIFF = "*** Delete File: src/gone.ts\n-a\n-b"
// `new file mode` is what routes a file into the "New files" section.
const CREATION_DIFF =
  "diff --git a/src/new.ts b/src/new.ts\nnew file mode 100644\n--- /dev/null\n+++ b/src/new.ts\n@@ -0,0 +1 @@\n+created line"
const DIFF_A =
  "diff --git a/src/a.ts b/src/a.ts\n@@ -1,2 +1,2 @@\n-alpha old\n+alpha new"
const DIFF_B =
  "diff --git a/src/b.ts b/src/b.ts\n@@ -1,2 +1,2 @@\n-bravo old\n+bravo new"

const changedFile = (
  id: string,
  path: string,
  diff: string | null
): FileChangeStat => ({ id, path, additions: 1, deletions: 1, diff })

// Only `sourceTurns[0].id` is read (it keys the diff tab); the file list comes
// from the mocked extractor, so a bare id is all this fixture needs.
const sourceTurns = [{ id: "reply-turn-1" }] as unknown as MessageTurn[]

function renderCard(files: FileChangeStat[], isResponseComplete = true) {
  mockExtract.mockReturnValue(files)
  return render(
    <ReplyArtifacts
      sourceTurns={sourceTurns}
      isResponseComplete={isResponseComplete}
    />
  )
}

// The "Files changed" section is collapsed by default — expand it so the
// per-file action buttons mount.
function expandChanged() {
  fireEvent.click(screen.getByText("title"))
}

describe("ReplyArtifacts — view diff action", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("opens the file's diff in the editor, keyed by the reply turn", () => {
    renderCard([
      {
        id: "f1",
        path: "src/a.ts",
        additions: 1,
        deletions: 1,
        diff: MODIFIED_DIFF,
      },
    ])
    expandChanged()

    fireEvent.click(screen.getByRole("button", { name: "viewDiff" }))

    expect(mockOpenDiff).toHaveBeenCalledWith(
      "src/a.ts",
      MODIFIED_DIFF,
      "reply-turn-1"
    )
  })

  it("falls back to the placeholder when the file has no diff data", () => {
    renderCard([
      { id: "f2", path: "src/b.ts", additions: 0, deletions: 0, diff: null },
    ])
    expandChanged()

    fireEvent.click(screen.getByRole("button", { name: "viewDiff" }))

    expect(mockOpenDiff).toHaveBeenCalledWith(
      "src/b.ts",
      "noDiffDataAvailable",
      "reply-turn-1"
    )
  })

  it("places View Diff to the left of Show-in-file-manager", () => {
    renderCard([
      {
        id: "f1",
        path: "src/a.ts",
        additions: 1,
        deletions: 1,
        diff: MODIFIED_DIFF,
      },
    ])
    expandChanged()

    const viewDiffBtn = screen.getByRole("button", { name: "viewDiff" })
    const revealBtn = screen.getByRole("button", { name: "revealInFolder" })
    // View Diff precedes the reveal button in document order.
    expect(
      viewDiffBtn.compareDocumentPosition(revealBtn) &
        Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
  })

  it("does not offer View Diff for a removed file (nothing to open)", () => {
    renderCard([
      {
        id: "f3",
        path: "src/gone.ts",
        additions: 0,
        deletions: 2,
        diff: DELETION_DIFF,
      },
    ])
    expandChanged()

    expect(
      screen.queryByRole("button", { name: "viewDiff" })
    ).not.toBeInTheDocument()
    // The removed file still renders its static destructive badge.
    expect(screen.getByText("remove")).toBeInTheDocument()
  })
})

describe("ReplyArtifacts — inline diff expansion", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  // Paired del/add rows render intraline word-diff spans, so one diff line's
  // text may be split across elements. Match the deepest node whose full
  // textContent is the line.
  const getDiffLine = (text: string) =>
    screen.getByText(
      (_, node) =>
        node?.textContent === text &&
        !Array.from(node?.children ?? []).some(
          (child) => child.textContent === text
        )
    )

  it("renders no diff rows until the file's toggle is clicked", () => {
    renderCard([changedFile("f1", "src/a.ts", DIFF_A)])
    expandChanged()

    // Lazy: nothing of the diff exists in the DOM before the first expand.
    expect(screen.queryByText("alpha new")).not.toBeInTheDocument()
    expect(screen.queryByText("alpha old")).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: "showInlineDiff" }))

    expect(getDiffLine("alpha new")).toBeInTheDocument()
    expect(getDiffLine("alpha old")).toBeInTheDocument()
  })

  it("collapses the panel when the open file's toggle is clicked again", () => {
    renderCard([changedFile("f1", "src/a.ts", DIFF_A)])
    expandChanged()

    fireEvent.click(screen.getByRole("button", { name: "showInlineDiff" }))
    expect(getDiffLine("alpha new")).toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: "hideInlineDiff" }))
    expect(screen.queryByText("alpha new")).not.toBeInTheDocument()
  })

  it("keeps at most one file expanded per section (accordion)", () => {
    renderCard([
      changedFile("f1", "src/a.ts", DIFF_A),
      changedFile("f2", "src/b.ts", DIFF_B),
    ])
    expandChanged()

    const [toggleA, toggleB] = screen.getAllByRole("button", {
      name: "showInlineDiff",
    })

    fireEvent.click(toggleA)
    expect(getDiffLine("alpha new")).toBeInTheDocument()
    expect(screen.queryByText("bravo new")).not.toBeInTheDocument()

    fireEvent.click(toggleB)
    // Opening the second file closes the first — one panel at a time.
    expect(screen.queryByText("alpha new")).not.toBeInTheDocument()
    expect(getDiffLine("bravo new")).toBeInTheDocument()
  })

  it("disables the toggle when the reply captured no diff for the file", () => {
    renderCard([
      { id: "f1", path: "src/a.ts", additions: 0, deletions: 0, diff: null },
    ])
    expandChanged()

    const toggle = screen.getByRole("button", { name: "noInlineDiff" })
    expect(toggle).toBeDisabled()
    // No "show/hide" affordance is offered at all for a diff-less file.
    expect(
      screen.queryByRole("button", { name: "showInlineDiff" })
    ).not.toBeInTheDocument()
  })

  it("expands inline diffs in the New files section too", () => {
    renderCard([
      {
        id: "n1",
        path: "src/new.ts",
        additions: 1,
        deletions: 0,
        diff: CREATION_DIFF,
      },
    ])
    // "New files" is open by default, so the toggle is already mounted.
    expect(screen.queryByText("created line")).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: "showInlineDiff" }))

    expect(screen.getByText("created line")).toBeInTheDocument()
  })

  it("leaves the workspace view-diff button in place next to the toggle", () => {
    renderCard([changedFile("f1", "src/a.ts", DIFF_A)])
    expandChanged()

    fireEvent.click(screen.getByRole("button", { name: "showInlineDiff" }))
    fireEvent.click(screen.getByRole("button", { name: "viewDiff" }))

    expect(mockOpenDiff).toHaveBeenCalledWith(
      "src/a.ts",
      DIFF_A,
      "reply-turn-1"
    )
  })

  it("parses nothing — and renders nothing — before the reply completes", () => {
    const { container } = renderCard(
      [changedFile("f1", "src/a.ts", DIFF_A)],
      false
    )

    expect(mockExtract).not.toHaveBeenCalled()
    expect(container).toBeEmptyDOMElement()
    expect(
      screen.queryByRole("button", { name: "showInlineDiff" })
    ).not.toBeInTheDocument()
  })
})
