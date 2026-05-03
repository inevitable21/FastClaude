import { useEffect, useState } from "react";
import { Dashboard } from "@/components/Dashboard";
import { Settings } from "@/components/Settings";
import { Onboarding } from "@/components/Onboarding";
import { History } from "@/components/History";
import { Toaster } from "@/components/ui/toaster";
import { onHotkeyFired, getFirstRun } from "@/lib/ipc";
import { UpdateBanner } from "@/components/UpdateBanner";
import { AuroraBackground } from "@/components/AuroraBackground";
import { TitleBar, BackButton, type View } from "@/components/TitleBar";
import { DashboardActions } from "@/components/DashboardActions";

export default function App() {
  const [view, setView] = useState<View | null>(null);
  const [launchOpen, setLaunchOpen] = useState(false);

  useEffect(() => {
    getFirstRun()
      .then((isFirst) => {
        if (isFirst) {
          setView("onboarding");
        } else {
          setView("dashboard");
          setLaunchOpen(true);
        }
      })
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

  const rightActions =
    view === "dashboard" ? (
      <DashboardActions
        onLaunch={() => setLaunchOpen(true)}
        onOpenHistory={() => setView("history")}
        onOpenSettings={() => setView("settings")}
      />
    ) : view === "settings" || view === "history" ? (
      <BackButton onClick={() => setView("dashboard")} />
    ) : null;

  return (
    <>
      <AuroraBackground />
      <div className="min-h-screen flex flex-col text-foreground relative z-10">
        <TitleBar view={view} rightActions={rightActions} />
        {view !== "onboarding" && <UpdateBanner />}
        <div className="flex-1 flex flex-col">
          {view === "onboarding" ? (
            <Onboarding onDone={() => setView("dashboard")} />
          ) : view === "dashboard" ? (
            <Dashboard
              launchOpen={launchOpen}
              setLaunchOpen={setLaunchOpen}
            />
          ) : view === "history" ? (
            <History />
          ) : (
            <Settings onBack={() => setView("dashboard")} />
          )}
        </div>
        <Toaster />
      </div>
    </>
  );
}
