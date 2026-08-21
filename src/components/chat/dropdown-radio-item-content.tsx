"use client"

interface DropdownRadioItemContentProps {
  label: string
  description?: string | null
  /** Single-line ellipsis instead of wrapping (launch-profile paths/URLs). */
  truncateDescription?: boolean
}

export function DropdownRadioItemContent({
  label,
  description,
  truncateDescription = false,
}: DropdownRadioItemContentProps) {
  const normalizedDescription = description?.trim()

  return (
    <div className="w-full min-w-0 pr-2" title={label}>
      <p className="truncate">{label}</p>
      {normalizedDescription ? (
        <p
          className={
            truncateDescription
              ? "text-muted-foreground mt-0.5 truncate text-xs leading-snug"
              : "text-muted-foreground mt-0.5 text-xs leading-snug whitespace-pre-wrap wrap-break-word"
          }
          title={truncateDescription ? normalizedDescription : undefined}
        >
          {normalizedDescription}
        </p>
      ) : null}
    </div>
  )
}
