import { Button, Dialog, TextField } from '../../ui'

export interface RenameDialogProps {
  open: boolean
  name: string
  error: string | null
  busy: boolean
  onNameChange: (value: string) => void
  onClose: () => void
  onSubmit: () => void
}

const FORM_ID = 'rename-form'

export function RenameDialog({
  open,
  name,
  error,
  busy,
  onNameChange,
  onClose,
  onSubmit,
}: RenameDialogProps) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="重命名"
      size="sm"
      footer={
        <>
          <Button onClick={onClose}>取消</Button>
          <Button
            type="submit"
            form={FORM_ID}
            variant="primary"
            data-testid="rename-submit"
            disabled={busy}
          >
            {busy ? '保存中…' : '保存'}
          </Button>
        </>
      }
    >
      <form
        id={FORM_ID}
        onSubmit={(event) => {
          event.preventDefault()
          onSubmit()
        }}
      >
        <TextField
          label="新名称"
          data-testid="rename-name"
          data-dialog-autofocus
          value={name}
          onChange={(event) => onNameChange(event.target.value)}
          error={error}
          autoComplete="off"
        />
      </form>
    </Dialog>
  )
}
