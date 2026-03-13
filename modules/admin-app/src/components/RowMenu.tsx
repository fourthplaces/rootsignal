import { type ReactNode } from "react";
import { Popover } from "@/components/Popover";

export type RowMenuItem = {
  label: string;
  onClick: () => void;
  variant?: "default" | "danger";
  disabled?: boolean;
};

type RowMenuProps = {
  items: RowMenuItem[];
  /** Optional content rendered below the menu items (e.g. error text) */
  footer?: ReactNode;
};

export function RowMenu({ items, footer }: RowMenuProps) {
  return (
    <Popover
      placement="bottom-end"
      content={(close) => (
        <div className="min-w-[140px]">
          {items.map((item) => (
            <button
              key={item.label}
              disabled={item.disabled}
              onClick={() => {
                item.onClick();
                close();
              }}
              className={`w-full text-left px-3 py-1.5 text-sm transition-colors disabled:opacity-50 ${
                item.variant === "danger"
                  ? "text-red-400 hover:bg-red-500/10"
                  : "text-foreground hover:bg-accent/50"
              }`}
            >
              {item.label}
            </button>
          ))}
          {footer}
        </div>
      )}
    >
      {(ref, props) => (
        <button
          ref={ref}
          {...props}
          className="p-1 rounded hover:bg-accent/50 text-muted-foreground hover:text-foreground"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
            <circle cx="8" cy="3" r="1.5" />
            <circle cx="8" cy="8" r="1.5" />
            <circle cx="8" cy="13" r="1.5" />
          </svg>
        </button>
      )}
    </Popover>
  );
}
