export function EmptyState({ onLaunch }: { onLaunch: () => void }) {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center">
      <h2 className="text-lg font-semibold">No running sessions</h2>
      <p className="text-sm text-muted-foreground mt-1">Launch one to get started.</p>
      <button
        onClick={onLaunch}
        className="mt-4 px-4 py-2 rounded bg-primary text-primary-foreground text-sm"
      >
        + Launch new session
      </button>
    </div>
  );
}
