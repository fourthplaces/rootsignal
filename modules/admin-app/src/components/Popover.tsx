import { useState, useCallback, type ReactNode, type RefCallback } from "react";
import {
  useFloating,
  autoUpdate,
  offset,
  flip,
  shift,
  useClick,
  useDismiss,
  useInteractions,
  FloatingPortal,
  type Placement,
} from "@floating-ui/react";

type PopoverProps = {
  placement?: Placement;
  content: (close: () => void) => ReactNode;
  children: (ref: RefCallback<HTMLElement>, props: Record<string, unknown>) => ReactNode;
};

export function Popover({ placement = "bottom-start", content, children }: PopoverProps) {
  const [open, setOpen] = useState(false);

  const { refs, floatingStyles, context } = useFloating({
    open,
    onOpenChange: setOpen,
    placement,
    middleware: [offset(4), flip(), shift({ padding: 8 })],
    whileElementsMounted: autoUpdate,
  });

  const click = useClick(context);
  const dismiss = useDismiss(context);
  const { getReferenceProps, getFloatingProps } = useInteractions([click, dismiss]);

  const close = useCallback(() => setOpen(false), []);

  return (
    <>
      {children(refs.setReference, getReferenceProps())}
      {open && (
        <FloatingPortal>
          <div
            ref={refs.setFloating}
            style={floatingStyles}
            className="z-[1100] bg-popover border border-border rounded-md shadow-lg py-1"
            {...getFloatingProps()}
          >
            {content(close)}
          </div>
        </FloatingPortal>
      )}
    </>
  );
}

/**
 * Popover anchored to a virtual element (bounding rect).
 * Useful when the trigger is rendered by a third-party library
 * and you can't attach a ref directly.
 */
type VirtualPopoverProps = {
  anchor: DOMRect | null;
  open: boolean;
  onClose: () => void;
  placement?: Placement;
  children: ReactNode;
};

export function VirtualPopover({ anchor, open, onClose, placement = "bottom-start", children }: VirtualPopoverProps) {
  const { refs, floatingStyles, context } = useFloating({
    open,
    onOpenChange: (v) => { if (!v) onClose(); },
    placement,
    middleware: [offset(4), flip(), shift({ padding: 8 })],
    whileElementsMounted: autoUpdate,
  });

  const dismiss = useDismiss(context);
  const { getFloatingProps } = useInteractions([dismiss]);

  // Sync virtual reference to anchor rect
  if (anchor) {
    refs.setReference({
      getBoundingClientRect: () => anchor,
    });
  }

  if (!open || !anchor) return null;

  return (
    <FloatingPortal>
      <div
        ref={refs.setFloating}
        style={floatingStyles}
        className="z-[1100] bg-popover border border-border rounded-md shadow-lg py-1"
        {...getFloatingProps()}
      >
        {children}
      </div>
    </FloatingPortal>
  );
}
