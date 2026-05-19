import { useEffect, useState } from "react";
import { Dashboard } from "@/components/Dashboard";
import { Settings } from "@/components/Settings";
import { Onboarding } from "@/components/Onboarding";
import { History } from "@/components/History";
import { ProjectSidebar } from "@/components/ProjectSidebar";
import { ProjectPane } from "@/components/ProjectPane";
import { Toaster } from "@/components/ui/toaster";
import {
  onHotkeyFired,
  getFirstRun,
  listProjects,
  onProjectChanged,
} from "@/lib/ipc";
import { UpdateBanner } from "@/components/UpdateBanner";
import { AuroraBackground } from "@/components/AuroraBackground";
import { TitleBar, BackButton, type View } from "@/components/TitleBar";
import { DashboardActions } from "@/components/DashboardActions";
import { LaunchDialog } from "@/components/LaunchDialog";
import type { Project } from "@/types";

export default function App() {
  const [view, setView] = useState<View | null>(null);
  const [launchOpen, setLaunchOpen] = useState(false);
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);

  useEffect(() => {
    getFirstRun()
      .then((isFirst) => setView(isFirst ? "onboarding" : "projects"))
      .catch(() => setView("projects"));
  }, []);

  useEffect(() => {
    listProjects().then(setProjects).catch(() => setProjects([]));
    const u: Array<() => void> = [];
    onProjectChanged(() => listProjects().then(setProjects).catch(() => {})).then((fn) => u.push(fn));
    return () => u.forEach((fn) => fn());
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    onHotkeyFired(() => {
      if (view !== "projects") setView("projects");
      setLaunchOpen(true);
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, [view]);

  if (view === null) return null;

  const rightActions =
    view === "projects" ? (
      <DashboardActions
        onLaunch={() => setLaunchOpen(true)}
        onOpenHistory={() => setView("history")}
        onOpenSettings={() => setView("settings")}
      />
    ) : view === "settings" || view === "history" || view === "dashboard" ? (
      <BackButton onClick={() => setView("projects")} />
    ) : null;

  const selectedProject =
    selectedProjectId === null ? null : projects.find((p) => p.id === selectedProjectId) ?? null;

  return (
    <>
      <AuroraBackground />
      <div className="min-h-screen flex flex-col text-foreground relative z-10">
        <TitleBar view={view} rightActions={rightActions} />
        {view !== "onboarding" && <UpdateBanner />}
        <div className="flex-1 flex">
          {view === "projects" && (
            <ProjectSidebar selectedId={selectedProjectId} onSelect={setSelectedProjectId} />
          )}
          <div className="flex-1 flex flex-col">
            {view === "onboarding" ? (
              <Onboarding onDone={() => setView("projects")} />
            ) : view === "projects" ? (
              selectedProject ? (
                <ProjectPane project={selectedProject} onLaunch={() => setLaunchOpen(true)} />
              ) : (
                <Dashboard launchOpen={false} setLaunchOpen={() => {}} />
              )
            ) : view === "dashboard" ? (
              <Dashboard launchOpen={launchOpen} setLaunchOpen={setLaunchOpen} />
            ) : view === "history" ? (
              <History />
            ) : (
              <Settings onBack={() => setView("projects")} />
            )}
          </div>
        </div>
        <LaunchDialog
          open={launchOpen}
          onOpenChange={setLaunchOpen}
          onLaunched={() => {
            // Project list may have a new entry from the auto-upsert; refetch.
            listProjects().then(setProjects).catch(() => {});
          }}
        />
        <Toaster />
      </div>
    </>
  );
}
