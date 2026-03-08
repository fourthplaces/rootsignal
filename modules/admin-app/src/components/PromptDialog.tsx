import { useState } from "react";

export function PromptDialog({
  title,
  description,
  onConfirm,
  onCancel,
  inputType = "text",
  confirmLabel,
  placeholder,
}: {
  title: string;
  description: string;
  onConfirm: (value: string) => void;
  onCancel: () => void;
  inputType?: "text" | "number" | "confirm";
  confirmLabel?: string;
  placeholder?: string;
}) {
  const [value, setValue] = useState("");
  const isConfirm = inputType === "confirm";
  const label = confirmLabel ?? (isConfirm ? "Delete" : "Apply");

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-card border border-border rounded-lg p-6 max-w-sm space-y-4">
        <h2 className="font-semibold">{title}</h2>
        <p className="text-sm text-muted-foreground">{description}</p>
        {!isConfirm && (
          <input
            type={inputType}
            step={inputType === "number" ? "0.01" : undefined}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && value) onConfirm(value);
              if (e.key === "Escape") onCancel();
            }}
            placeholder={placeholder}
            className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
            autoFocus
          />
        )}
        <div className="flex gap-2 justify-end">
          <button
            onClick={onCancel}
            className="px-3 py-1.5 rounded-md border border-border text-sm text-muted-foreground hover:text-foreground"
          >
            Cancel
          </button>
          <button
            onClick={() => onConfirm(value)}
            disabled={!isConfirm && !value}
            className={`px-3 py-1.5 rounded-md text-sm text-white disabled:opacity-50 ${
              isConfirm ? "bg-red-600 hover:bg-red-700" : "bg-primary hover:bg-primary/90"
            }`}
          >
            {label}
          </button>
        </div>
      </div>
    </div>
  );
}
