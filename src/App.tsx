import { useEffect, useState } from "react";
import { Dashboard } from "@/components/Dashboard";
import { Settings } from "@/components/Settings";
import { Toaster } from "@/components/ui/toaster";
import { onHotkeyFired } from "@/lib/ipc";

type View = "dashboard" | "settings";

export default function App() {
  const [view, setView] = useState<View>("dashboard");
  const [launchOpen, setLaunchOpen] = useState(false);

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

  return (
    <div className="min-h-screen flex flex-col bg-background text-foreground">
      <div className="flex-1 flex flex-col">
        {view === "dashboard" ? (
          <Dashboard
            onOpenSettings={() => setView("settings")}
            launchOpen={launchOpen}
            setLaunchOpen={setLaunchOpen}
          />
        ) : (
          <Settings onBack={() => setView("dashboard")} />
        )}
      </div>
      <Toaster />
    </div>
  );
}
