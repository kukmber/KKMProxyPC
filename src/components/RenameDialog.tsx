import { useEffect, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Input,
} from "@fluentui/react-components";
import { api, Profile } from "../api";

export function RenameDialog(props: { profile: Profile | null; onClose: () => void; onDone: () => void }) {
  const [name, setName] = useState("");
  useEffect(() => setName(props.profile?.name ?? ""), [props.profile]);

  return (
    <Dialog open={!!props.profile} onOpenChange={(_, d) => !d.open && props.onClose()}>
      <DialogSurface style={{ maxWidth: 420 }}>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            if (!props.profile || !name.trim()) return;
            await api.renameProfile(props.profile.id, name);
            props.onDone();
          }}
        >
          <DialogBody>
            <DialogTitle>Переименовать</DialogTitle>
            <DialogContent>
              <Input autoFocus value={name} onChange={(_, d) => setName(d.value)} style={{ width: "100%" }} />
            </DialogContent>
            <DialogActions>
              <Button onClick={props.onClose}>Отмена</Button>
              <Button type="submit" appearance="primary" disabled={!name.trim()}>
                Сохранить
              </Button>
            </DialogActions>
          </DialogBody>
        </form>
      </DialogSurface>
    </Dialog>
  );
}
