export function FormError({ message }: { message: string | null }) {
  if (!message) {
    return null
  }
  return (
    <p
      role="alert"
      className="rounded-control bg-danger-50 px-3 py-2 text-sm leading-6 text-danger-600"
    >
      {message}
    </p>
  )
}

export function FormNotice({ message }: { message: string | null }) {
  if (!message) {
    return null
  }
  return (
    <p className="rounded-control bg-success-50 px-3 py-2 text-sm leading-6 text-success-600">
      {message}
    </p>
  )
}
