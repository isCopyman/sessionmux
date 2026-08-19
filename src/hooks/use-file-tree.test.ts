import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { WorkspaceFileEntry } from "@/lib/types"

import { useFileTree } from "./use-file-tree"

const mocks = vi.hoisted(() => ({
  listWorkspaceFiles: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  listWorkspaceFiles: (...args: unknown[]) =>
    mocks.listWorkspaceFiles(...args),
}))

function entry(
  path: string,
  root: string,
  kind: "file" | "dir" = "file"
): WorkspaceFileEntry {
  return { name: path.split("/").pop() ?? path, path, kind, root }
}

describe("useFileTree", () => {
  beforeEach(() => {
    mocks.listWorkspaceFiles.mockReset().mockResolvedValue([])
  })

  it("does not fetch while disabled or without a folder path", () => {
    renderHook(() => useFileTree({ folderPath: undefined, enabled: true }))
    renderHook(() => useFileTree({ folderPath: "/repo", enabled: false }))
    expect(mocks.listWorkspaceFiles).not.toHaveBeenCalled()
  })

  it("fetches the single root with no extra paths (plain composer, unchanged behavior)", async () => {
    mocks.listWorkspaceFiles.mockResolvedValue([entry("a.ts", "/repo")])
    const { result } = renderHook(() =>
      useFileTree({ folderPath: "/repo", enabled: true })
    )
    await waitFor(() => expect(result.current.loaded).toBe(true))

    expect(mocks.listWorkspaceFiles).toHaveBeenCalledWith("/repo", undefined)
    expect(result.current.allFiles).toEqual([
      {
        name: "a.ts",
        relativePath: "a.ts",
        kind: "file",
        lowerPath: "a.ts",
        lowerName: "a.ts",
        root: "/repo",
      },
    ])
  })

  it("merges the bound folder and additional paths into a single call", async () => {
    mocks.listWorkspaceFiles.mockResolvedValue([
      entry("main.rs", "/repo"),
      entry("notes.md", "/extra"),
    ])
    const { result } = renderHook(() =>
      useFileTree({
        folderPath: "/repo",
        additionalPaths: ["/extra"],
        enabled: true,
      })
    )
    await waitFor(() => expect(result.current.loaded).toBe(true))

    expect(mocks.listWorkspaceFiles).toHaveBeenCalledTimes(1)
    expect(mocks.listWorkspaceFiles).toHaveBeenCalledWith("/repo", ["/extra"])
    expect(result.current.allFiles.map((f) => f.root)).toEqual([
      "/repo",
      "/extra",
    ])
  })

  it("still searches additional paths when there is no bound folder", async () => {
    mocks.listWorkspaceFiles.mockResolvedValue([entry("notes.md", "/extra")])
    const { result } = renderHook(() =>
      useFileTree({
        folderPath: undefined,
        additionalPaths: ["/extra"],
        enabled: true,
      })
    )
    await waitFor(() => expect(result.current.loaded).toBe(true))

    // The first (only) resolved root becomes the `path` argument; there is
    // nothing left over for `extraPaths`.
    expect(mocks.listWorkspaceFiles).toHaveBeenCalledWith("/extra", undefined)
    expect(result.current.allFiles).toHaveLength(1)
  })

  it("does not refetch when the same root combination recurs, even as a new array instance", async () => {
    mocks.listWorkspaceFiles.mockResolvedValue([entry("a.ts", "/repo")])
    const { rerender } = renderHook(
      (props: { additionalPaths: string[] }) =>
        useFileTree({
          folderPath: "/repo",
          additionalPaths: props.additionalPaths,
          enabled: true,
        }),
      { initialProps: { additionalPaths: ["/extra"] } }
    )
    await waitFor(() =>
      expect(mocks.listWorkspaceFiles).toHaveBeenCalledTimes(1)
    )

    // A fresh array with the same contents (as a memoized caller might still
    // produce without deep-comparing) must not trigger a second fetch.
    rerender({ additionalPaths: ["/extra"] })
    await Promise.resolve()
    expect(mocks.listWorkspaceFiles).toHaveBeenCalledTimes(1)
  })

  it("refetches when the additional-path list actually changes", async () => {
    mocks.listWorkspaceFiles.mockResolvedValue([entry("a.ts", "/repo")])
    const { rerender } = renderHook(
      (props: { additionalPaths: string[] }) =>
        useFileTree({
          folderPath: "/repo",
          additionalPaths: props.additionalPaths,
          enabled: true,
        }),
      { initialProps: { additionalPaths: ["/extra"] } }
    )
    await waitFor(() =>
      expect(mocks.listWorkspaceFiles).toHaveBeenCalledTimes(1)
    )

    rerender({ additionalPaths: ["/extra", "/another"] })
    await waitFor(() =>
      expect(mocks.listWorkspaceFiles).toHaveBeenCalledTimes(2)
    )
    expect(mocks.listWorkspaceFiles).toHaveBeenLastCalledWith("/repo", [
      "/extra",
      "/another",
    ])
  })

  it("clears a previously-loaded file list when a later fetch rejects", async () => {
    mocks.listWorkspaceFiles.mockResolvedValue([entry("a.ts", "/a")])
    const { result, rerender } = renderHook(
      (props: { folderPath: string }) =>
        useFileTree({ folderPath: props.folderPath, enabled: true }),
      { initialProps: { folderPath: "/a" } }
    )
    await waitFor(() => expect(result.current.allFiles).toHaveLength(1))

    mocks.listWorkspaceFiles.mockRejectedValue(new Error("boom"))
    rerender({ folderPath: "/b" })
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.allFiles).toEqual([])
  })

  it("reset() clears the cache so a later re-enable triggers a fresh load", async () => {
    mocks.listWorkspaceFiles.mockResolvedValue([entry("a.ts", "/repo")])
    const { result, rerender } = renderHook(
      (props: { enabled: boolean }) =>
        useFileTree({ folderPath: "/repo", enabled: props.enabled }),
      { initialProps: { enabled: true } }
    )
    await waitFor(() => expect(result.current.loaded).toBe(true))
    expect(mocks.listWorkspaceFiles).toHaveBeenCalledTimes(1)

    // Clearing the cache alone does not itself re-invoke the effect (nothing
    // it depends on changed yet) — `reset()`'s contract is to make the *next*
    // dependency change (here, re-enabling) see a cache miss instead of the
    // stale "already loaded for this key" hit.
    act(() => {
      result.current.reset()
    })
    expect(result.current.allFiles).toEqual([])

    rerender({ enabled: false })
    rerender({ enabled: true })
    await waitFor(() =>
      expect(mocks.listWorkspaceFiles).toHaveBeenCalledTimes(2)
    )
  })
})
