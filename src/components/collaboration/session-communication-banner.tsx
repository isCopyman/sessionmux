"use client"

import { useState } from "react"
import {
  ChevronDown,
  ChevronUp,
  Inbox,
  MessageSquareMore,
  X,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { Button } from "@/components/ui/button"
import { useCollaborationFeed } from "@/hooks/use-collaboration-feed"
import type { CollaborationDelivery } from "@/lib/types"

interface SessionCommunicationBannerProps {
  conversationId: number | null
}

function participantLabel(
  participant: CollaborationDelivery["source"],
  untitled: (values: { id: number }) => string
) {
  return (
    participant.title?.trim() || untitled({ id: participant.conversationId })
  )
}

export function SessionCommunicationBanner({
  conversationId,
}: SessionCommunicationBannerProps) {
  const t = useTranslations("Collaboration")
  const [expanded, setExpanded] = useState(false)
  const { feed, hydrated, markSeen, dismiss } =
    useCollaborationFeed(conversationId)
  const total = feed.inbound.length + feed.outbound.length
  if (!hydrated || total === 0) return null

  const unreadIds = feed.inbound
    .filter(
      (delivery) => delivery.uiSeenAt == null && delivery.state !== "dismissed"
    )
    .map((delivery) => delivery.id)

  return (
    <section className="border-b border-border/60 bg-background/80 backdrop-blur-sm">
      <div className="mx-auto max-w-3xl px-4 py-1.5">
        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          className="flex w-full items-center gap-2 rounded-md px-1 py-1 text-left text-xs transition-colors hover:bg-muted/50"
        >
          <MessageSquareMore className="h-4 w-4 text-muted-foreground" />
          <span className="font-medium">{t("panelTitle")}</span>
          <span className="text-muted-foreground">
            {t("messageCount", { count: total })}
          </span>
          {feed.unreadCount > 0 ? (
            <span className="rounded-full bg-primary px-1.5 py-0.5 text-[10px] font-medium text-primary-foreground">
              {t("unreadCount", { count: feed.unreadCount })}
            </span>
          ) : null}
          <span className="ml-auto text-muted-foreground">
            {expanded ? (
              <ChevronUp className="h-4 w-4" />
            ) : (
              <ChevronDown className="h-4 w-4" />
            )}
          </span>
        </button>

        {expanded ? (
          <div className="space-y-3 pb-2 pt-1">
            {unreadIds.length > 0 ? (
              <div className="flex justify-end">
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-7 text-xs"
                  onClick={() => void markSeen(unreadIds)}
                >
                  {t("markAllRead")}
                </Button>
              </div>
            ) : null}

            {feed.inbound.length > 0 ? (
              <div className="space-y-1.5">
                <p className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                  <Inbox className="h-3.5 w-3.5" />
                  {t("inbound")}
                </p>
                {feed.inbound.map((delivery) => (
                  <article
                    key={delivery.id}
                    className="rounded-lg border bg-card/70 px-3 py-2 text-sm"
                  >
                    <div className="flex items-start gap-2">
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                          <span className="font-medium">
                            {t("from", {
                              name: participantLabel(
                                delivery.source,
                                (values) => t("untitled", values)
                              ),
                            })}
                          </span>
                          <span className="text-muted-foreground">
                            {delivery.source.agentType || t("unknownHarness")}
                          </span>
                          {delivery.uiSeenAt == null &&
                          delivery.state !== "dismissed" ? (
                            <span className="h-1.5 w-1.5 rounded-full bg-primary" />
                          ) : null}
                        </div>
                        <p className="mt-1 whitespace-pre-wrap break-words leading-relaxed">
                          {delivery.body}
                        </p>
                        <p className="mt-1 text-[11px] text-muted-foreground">
                          {delivery.state === "dismissed"
                            ? t("stateDismissed")
                            : t("stateStoreOnly")}
                        </p>
                      </div>
                      {delivery.state === "pending" ? (
                        <Button
                          type="button"
                          size="icon"
                          variant="ghost"
                          className="h-7 w-7 shrink-0"
                          title={t("dismiss")}
                          aria-label={t("dismiss")}
                          onClick={() => void dismiss(delivery.id)}
                        >
                          <X className="h-3.5 w-3.5" />
                        </Button>
                      ) : null}
                    </div>
                  </article>
                ))}
              </div>
            ) : null}

            {feed.outbound.length > 0 ? (
              <div className="space-y-1.5">
                <p className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                  {t("outbound")}
                </p>
                {feed.outbound.map((delivery) => (
                  <article
                    key={delivery.id}
                    className="rounded-lg border bg-muted/30 px-3 py-2 text-sm"
                  >
                    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                      <span className="font-medium">
                        {t("to", {
                          name: participantLabel(delivery.target, (values) =>
                            t("untitled", values)
                          ),
                        })}
                      </span>
                      <span className="text-muted-foreground">
                        {delivery.state === "failed"
                          ? t("stateFailed")
                          : delivery.state === "dismissed"
                            ? t("stateDismissedByTarget")
                            : delivery.uiSeenAt
                              ? t("stateSeen")
                              : t("stateDelivered")}
                      </span>
                    </div>
                    <p className="mt-1 whitespace-pre-wrap break-words leading-relaxed">
                      {delivery.body}
                    </p>
                  </article>
                ))}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
    </section>
  )
}
