"use client"

import type { ComponentProps } from "react"

import { openUrl } from "@/lib/platform"

/** External link that works in both a browser and a Tauri webview. */
export function BrowserLink({
  href,
  onClick,
  children,
  ...props
}: Omit<ComponentProps<"a">, "href" | "target"> & { href: string }) {
  return (
    <a
      {...props}
      href={href}
      target="_blank"
      rel="noreferrer"
      onClick={(event) => {
        onClick?.(event)
        if (event.defaultPrevented) return
        event.preventDefault()
        void openUrl(href)
      }}
    >
      {children}
    </a>
  )
}
