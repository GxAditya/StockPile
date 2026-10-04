import { Modal } from "./ui";
import { AddStockForm } from "./AddStockForm";

/**
 * Dialog wrapper around the shared receive form. Inventory keeps this for its in-context "add
 * stock" button; the sidebar entry goes to the standalone `/add` page. Both render the same
 * `AddStockForm`, so there is still only one implementation of receiving stock.
 */
export function AddStockModal({
  open,
  onClose,
  onDone,
  defaultProductId,
}: {
  open: boolean;
  onClose: () => void;
  onDone: () => void;
  defaultProductId?: number;
}) {
  if (!open) return null;
  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Receive stock"
      subtitle="Add a batch to an existing product, or create a product and its first batch together."
      width={620}
      footer={
        <button onClick={onClose} className="btn btn-ghost">
          Close
        </button>
      }
    >
      <AddStockForm
        mode="existing"
        onModeChange={() => undefined}
        onDone={() => {
          onDone();
          onClose();
        }}
        defaultProductId={defaultProductId}
      />
    </Modal>
  );
}
