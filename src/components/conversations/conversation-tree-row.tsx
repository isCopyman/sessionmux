"use client"

import type { ButtonHTMLAttributes, CSSProperties, ReactNode, Ref } from "react"
import { CheckSquare, Square } from "lucide-react"

import { cn } from "@/lib/utils"

type DataAttributes = {
  [key: `data-${string}`]: string | number | boolean | undefined
}

export interface ConversationTreeRowProps {
  depth: number
  selected: boolean
  checked: boolean
  selectionActive: boolean
  selectAriaLabel: string
  selectButtonProps?: Omit<
    ButtonHTMLAttributes<HTMLButtonElement>,
    "aria-label" | "aria-pressed" | "onClick" | "type"
  > &
    DataAttributes
  onToggleSelection: () => void
  mainRef?: Ref<HTMLButtonElement>
  mainButtonProps: Omit<ButtonHTMLAttributes<HTMLButtonElement>, "type"> &
    DataAttributes
  leading: ReactNode
  title: ReactNode
  trailing?: ReactNode
  rowClassName?: string
  rowStyle?: CSSProperties
  mainClassName?: string
}

/**
 * Shared visual/interaction shell for every conversational item in the
 * Collection tree. Session and Room keep separate identities, runtimes and
 * context menus; this component owns only the behavior that must not drift:
 * selection affordance, focused/checked styling, indentation and row geometry.
 */
export function ConversationTreeRow({
  depth,
  selected,
  checked,
  selectionActive,
  selectAriaLabel,
  selectButtonProps,
  onToggleSelection,
  mainRef,
  mainButtonProps,
  leading,
  title,
  trailing,
  rowClassName,
  rowStyle,
  mainClassName,
}: ConversationTreeRowProps) {
  const { className: selectClassName, ...restSelectProps } =
    selectButtonProps ?? {}
  const { className: buttonClassName, ...restMainProps } = mainButtonProps

  return (
    <div
      className={cn(
        "group flex min-w-0 items-center rounded-md hover:bg-sidebar-accent",
        selected &&
          "bg-primary/8 text-primary ring-1 ring-inset ring-primary/30",
        checked && "bg-sidebar-primary/12 ring-1 ring-inset ring-primary/25",
        rowClassName
      )}
      style={{
        paddingInlineStart: `${0.75 + depth * 0.75}rem`,
        ...rowStyle,
      }}
    >
      <button
        type="button"
        tabIndex={-1}
        aria-pressed={checked}
        aria-label={selectAriaLabel}
        {...restSelectProps}
        className={cn(
          "flex h-4 w-0 shrink-0 items-center justify-center overflow-hidden rounded-sm text-muted-foreground hover:text-foreground",
          "pointer-events-none opacity-0",
          "group-hover:pointer-events-auto group-hover:h-4 group-hover:w-4 group-hover:opacity-100",
          (selectionActive || checked) &&
            "pointer-events-auto h-4 w-4 opacity-100",
          selectClassName
        )}
        onPointerDown={(event) => event.stopPropagation()}
        onClick={(event) => {
          event.preventDefault()
          event.stopPropagation()
          onToggleSelection()
        }}
      >
        {checked ? (
          <CheckSquare className="h-3.5 w-3.5 text-primary" />
        ) : (
          <Square className="h-3.5 w-3.5" />
        )}
      </button>
      <button
        ref={mainRef}
        type="button"
        {...restMainProps}
        className={cn(
          "flex h-7 min-w-0 flex-1 items-center gap-1.5 pe-2 text-start text-xs",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
          (selectionActive || checked) && "ps-1",
          "group-hover:ps-1",
          mainClassName,
          buttonClassName
        )}
      >
        {leading}
        <span className="min-w-0 flex-1 truncate">{title}</span>
        {trailing}
      </button>
    </div>
  )
}
