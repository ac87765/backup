import { Button, Dialog, TextField } from '../../ui'

export interface CreateFolderDialogProps {
  open: boolean
  name: string
  error: string | null
  busy: boolean
  onNameChange: (value: string) => void
  onClose: () => void
  onSubmit: () => void
}

const FORM_ID = 'create-folder-form'

export function CreateFolderDialog({
  open,
  name,
  error,
  busy,
  onNameChange,
  onClose,
  onSubmit,
}: CreateFolderDialogProps) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="新建文件夹"
      size="sm"
      footer={
        <>
          <Button onClick={onClose}>取消</Button>
          <Button
            type="submit"
            form={FORM_ID}
            variant="primary"
            data-testid="create-folder-submit"
            disabled={busy}
          >
            {busy ? '创建中…' : '创建'}
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
          label="文件夹名称"
          data-testid="folder-name"
          data-dialog-autofocus
          value={name}
          onChange={(event) => onNameChange(event.target.value)}
          error={error}
          placeholder="例如：合同"
          autoComplete="off"
        />
      </form>
    </Dialog>
  )
}
