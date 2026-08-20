import { describe, expect, it } from "vitest"

import { roomTabFolderId } from "./open-room"

describe("roomTabFolderId", () => {
  it("uses the Room's own root folder when the Room has one", () => {
    expect(roomTabFolderId({ rootFolderId: 9 }, [{ id: 1 }, { id: 2 }])).toBe(9)
  })

  it("does not bind to a creator Session folder", () => {
    expect(roomTabFolderId({ rootFolderId: null }, [{ id: 4 }])).toBe(4)
    expect(roomTabFolderId({}, [])).toBe(1)
  })
})
