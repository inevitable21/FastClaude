import { useEffect, useState } from "react";
import { getUsageSummary, onUsageUpdated } from "@/lib/ipc";
import type { UsageSummary } from "@/types";

const SECS_PER_DAY = 86_400;

function startOfDayEpoch(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return Math.floor(d.getTime() / 1000);
}

function startOfWeekEpoch(): number {
  return startOfDayEpoch() - 6 * SECS_PER_DAY;
}

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

function fmtCost(n: number): string {
  return `$${n.toFixed(2)}`;
}

export function UsageStrip() {
  const [today, setToday] = useState<UsageSummary | null>(null);
  const [week, setWeek] = useState<UsageSummary | null>(null);

  function refresh() {
    getUsageSummary(startOfDayEpoch()).then(setToday).catch(() => {});
    getUsageSummary(startOfWeekEpoch()).then(setWeek).catch(() => {});
  }

  useEffect(() => {
    refresh();
    let unlisten: (() => void) | null = null;
    onUsageUpdated(refresh).then((fn) => {
      unlisten = fn;
    });
    const t = setInterval(refresh, 30_000);
    return () => {
      unlisten?.();
      clearInterval(t);
    };
  }, []);

  if (!today || !week) return null;
  const totalTokens =
    today.tokens_in + today.tokens_out + today.tokens_cache_read + today.tokens_cache_write;

  return (
    <div className="flex gap-6 px-4 py-2 text-xs bg-muted/30 border-t border-border">
      <div>
        <span className="text-muted-foreground">Today:</span>{" "}
        <strong>{fmtCost(today.cost_usd)}</strong>
      </div>
      <div>
        <span className="text-muted-foreground">This week:</span>{" "}
        <strong>{fmtCost(week.cost_usd)}</strong>
      </div>
      <div>
        <span className="text-muted-foreground">Today tokens:</span>{" "}
        <strong>{fmtTokens(totalTokens)}</strong>
      </div>
    </div>
  );
}
