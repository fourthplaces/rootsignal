import { useState } from "react";
import { useQuery, useMutation } from "@apollo/client";
import { BUDGET_STATUS } from "@/graphql/queries";
import { SET_BUDGET } from "@/graphql/mutations";

interface BudgetData {
  budgetStatus: {
    dailyLimitCents: number;
    spentTodayCents: number;
    remainingCents: number;
    perRunMaxCents: number;
  };
}

function limitToDisplay(cents: number): string {
  if (cents === 0) return "Unlimited";
  return `$${(cents / 100).toFixed(2)}`;
}

function dollarsToDisplay(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`;
}

function ProgressBar({ spent, limit }: { spent: number; limit: number }) {
  if (limit === 0) return <span className="text-xs text-muted-foreground">No limit set</span>;
  const pct = Math.min((spent / limit) * 100, 100);
  const color = pct > 90 ? "bg-red-500" : pct > 70 ? "bg-yellow-500" : "bg-green-500";
  return (
    <div className="w-full bg-muted rounded-full h-2">
      <div className={`${color} h-2 rounded-full transition-all`} style={{ width: `${pct}%` }} />
    </div>
  );
}

function EditableLimit({
  currentCents,
  onSave,
}: {
  currentCents: number;
  onSave: (cents: number) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(String(currentCents / 100));

  const handleSave = () => {
    const cents = Math.round(parseFloat(value) * 100);
    if (!isNaN(cents) && cents >= 0) {
      onSave(cents);
    }
    setEditing(false);
  };

  if (editing) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-sm text-muted-foreground">$</span>
        <input
          type="number"
          min="0"
          step="0.01"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSave()}
          className="w-24 px-2 py-1 text-sm rounded border border-input bg-background"
          autoFocus
        />
        <button onClick={handleSave} className="text-xs px-2 py-1 rounded bg-accent hover:bg-accent/80">
          Save
        </button>
        <button onClick={() => setEditing(false)} className="text-xs px-2 py-1 text-muted-foreground">
          Cancel
        </button>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2">
      <span className="font-mono text-lg">{limitToDisplay(currentCents)}</span>
      <button
        onClick={() => {
          setValue(String(currentCents / 100));
          setEditing(true);
        }}
        className="text-xs px-2 py-1 rounded border border-input hover:bg-accent/50 text-muted-foreground"
      >
        Edit
      </button>
    </div>
  );
}

export function BudgetPage() {
  const { data, loading, refetch } = useQuery<BudgetData>(BUDGET_STATUS);
  const [setBudget] = useMutation(SET_BUDGET);

  const save = async (dailyLimitCents: number, perRunMaxCents: number) => {
    await setBudget({ variables: { dailyLimitCents, perRunMaxCents } });
    refetch();
  };

  if (loading || !data) {
    return <p className="text-muted-foreground">Loading budget data...</p>;
  }

  const b = data.budgetStatus;

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-semibold">Budget</h1>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Daily Budget */}
        <div className="rounded-lg border border-border bg-card p-4 space-y-3">
          <h2 className="text-sm font-medium text-muted-foreground uppercase tracking-wide">
            Daily Budget
          </h2>
          <EditableLimit
            currentCents={b.dailyLimitCents}
            onSave={(cents) => save(cents, b.perRunMaxCents)}
          />
          <div className="text-sm space-y-1">
            <div className="flex justify-between">
              <span className="text-muted-foreground">Spent today</span>
              <span className="font-mono">{dollarsToDisplay(b.spentTodayCents)}</span>
            </div>
            {b.dailyLimitCents > 0 && (
              <div className="flex justify-between">
                <span className="text-muted-foreground">Remaining</span>
                <span className="font-mono">{dollarsToDisplay(b.remainingCents)}</span>
              </div>
            )}
          </div>
          <ProgressBar spent={b.spentTodayCents} limit={b.dailyLimitCents} />
        </div>

        {/* Per-Run Max */}
        <div className="rounded-lg border border-border bg-card p-4 space-y-3">
          <h2 className="text-sm font-medium text-muted-foreground uppercase tracking-wide">
            Per-Run Max
          </h2>
          <EditableLimit
            currentCents={b.perRunMaxCents}
            onSave={(cents) => save(b.dailyLimitCents, cents)}
          />
          <p className="text-xs text-muted-foreground">
            Maximum budget for any single run. 0 = unlimited.
          </p>
        </div>
      </div>
    </div>
  );
}
