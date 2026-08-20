import { renderHook } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { useTranscriptInfiniteScroll } from "./use-transcript-infinite-scroll"

describe("useTranscriptInfiniteScroll", () => {
  it("derives searchingOlderHistory from find + remaining older pages", () => {
    const onLoadOlder = vi.fn()
    const { result, rerender } = renderHook(
      (props: Parameters<typeof useTranscriptInfiniteScroll>[0]) =>
        useTranscriptInfiniteScroll(props),
      {
        initialProps: {
          hasOlder: true,
          isLoading: false,
          onLoadOlder,
          isActive: true,
          findOpen: true,
          findQuery: "alpha",
        },
      }
    )
    expect(result.current.searchingOlderHistory).toBe(true)

    rerender({
      hasOlder: false,
      isLoading: false,
      onLoadOlder,
      isActive: true,
      findOpen: true,
      findQuery: "alpha",
    })
    expect(result.current.searchingOlderHistory).toBe(false)
  })

  it("keeps searchingOlderHistory true while an older-page request is in flight", () => {
    const onLoadOlder = vi.fn()
    const { result } = renderHook(() =>
      useTranscriptInfiniteScroll({
        hasOlder: false,
        isLoading: true,
        onLoadOlder,
        isActive: true,
        findOpen: true,
        findQuery: "alpha",
      })
    )
    expect(result.current.searchingOlderHistory).toBe(true)
  })

  it("pages older history while a non-empty find query is active", () => {
    const onLoadOlder = vi.fn()
    renderHook(() =>
      useTranscriptInfiniteScroll({
        hasOlder: true,
        isLoading: false,
        onLoadOlder,
        isActive: true,
        findOpen: true,
        findQuery: "needle",
      })
    )
    expect(onLoadOlder).toHaveBeenCalledTimes(1)
  })

  it("does not page while an older-page request is already in flight", () => {
    const onLoadOlder = vi.fn()
    renderHook(() =>
      useTranscriptInfiniteScroll({
        hasOlder: true,
        isLoading: true,
        onLoadOlder,
        isActive: true,
        findOpen: true,
        findQuery: "needle",
      })
    )
    expect(onLoadOlder).not.toHaveBeenCalled()
  })

  it("retriggers after a page settles if older history still remains", () => {
    const onLoadOlder = vi.fn()
    const { rerender } = renderHook(
      (props: Parameters<typeof useTranscriptInfiniteScroll>[0]) =>
        useTranscriptInfiniteScroll(props),
      {
        initialProps: {
          hasOlder: true,
          isLoading: true,
          onLoadOlder,
          isActive: true,
          findOpen: true,
          findQuery: "needle",
        },
      }
    )
    expect(onLoadOlder).not.toHaveBeenCalled()
    rerender({
      hasOlder: true,
      isLoading: false,
      onLoadOlder,
      isActive: true,
      findOpen: true,
      findQuery: "needle",
    })
    expect(onLoadOlder).toHaveBeenCalledTimes(1)
  })

  it("does not page when find is closed or the query is empty", () => {
    const onLoadOlder = vi.fn()
    const { rerender } = renderHook(
      (props: Parameters<typeof useTranscriptInfiniteScroll>[0]) =>
        useTranscriptInfiniteScroll(props),
      {
        initialProps: {
          hasOlder: true,
          isLoading: false,
          onLoadOlder,
          isActive: true,
          findOpen: false,
          findQuery: "needle",
        },
      }
    )
    expect(onLoadOlder).not.toHaveBeenCalled()
    rerender({
      hasOlder: true,
      isLoading: false,
      onLoadOlder,
      isActive: true,
      findOpen: true,
      findQuery: "",
    })
    expect(onLoadOlder).not.toHaveBeenCalled()
  })

  it("does not page when the host surface is inactive", () => {
    const onLoadOlder = vi.fn()
    renderHook(() =>
      useTranscriptInfiniteScroll({
        hasOlder: true,
        isLoading: false,
        onLoadOlder,
        isActive: false,
        findOpen: true,
        findQuery: "needle",
      })
    )
    expect(onLoadOlder).not.toHaveBeenCalled()
  })
})
