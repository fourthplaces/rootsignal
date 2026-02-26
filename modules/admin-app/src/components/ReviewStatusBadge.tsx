const STATUS_STYLES: Record<string, string> = {
  staged: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  live: "bg-green-500/10 text-green-400 border-green-500/20",
  rejected: "bg-red-500/10 text-red-400 border-red-500/20",
};

export function ReviewStatusBadge({
  status,
  wasCorrected,
}: {
  status: string;
  wasCorrected?: boolean;
}) {
  if (status === "live" && wasCorrected) {
    return (
      <span className="px-2 py-0.5 rounded text-xs border bg-blue-500/10 text-blue-400 border-blue-500/20">
        corrected
      </span>
    );
  }

  const style = STATUS_STYLES[status] ?? STATUS_STYLES.staged;
  return (
    <span className={`px-2 py-0.5 rounded text-xs border ${style}`}>
      {status}
    </span>
  );
}
