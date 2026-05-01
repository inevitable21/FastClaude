import { useEffect, useState } from "react";
import { Dashboard } from "@/components/Dashboard";
import { Settings } from "@/components/Settings";
import { Onboarding } from "@/components/Onboarding";
import { History } from "@/components/History";
import { Toaster } from "@/components/ui/toaster";
import { onHotkeyFired, getFirstRun } from "@/lib/ipc";
import { UpdateBanner } from "@/components/UpdateBanner";

type View = "dashboard" | "settings" | "onboarding" | "history";

export default function App() {
  const [view, setView] = useState<View | null>(null);
  const [launchOpen, setLaunchOpen] = useState(false);

  useEffect(() => {
    getFirstRun()
      .then((isFirst) => setView(isFirst ? "onboarding" : "dashboard"))
      .catch(() => setView("dashboard"));
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    onHotkeyFired(() => {
      setView("dashboard");
      setLaunchOpen(true);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  if (view === null) return null;

  return (
    <div className="min-h-screen flex flex-col bg-background text-foreground">
      {view !== "onboarding" && <UpdateBanner />}
      <div className="flex-1 flex flex-col">
        {view === "onboarding" ? (
          <Onboarding onDone={() => setView("dashboard")} />
        ) : view === "dashboard" ? (
          <Dashboard
            onOpenSettings={() => setView("settings")}
            onOpenHistory={() => setView("history")}
            launchOpen={launchOpen}
            setLaunchOpen={setLaunchOpen}
          />
        ) : view === "history" ? (
          <History onBack={() => setView("dashboard")} />
        ) : (
          <Settings onBack={() => setView("dashboard")} />
        )}
      </div>
      <Toaster />
    </div>
  );
}
