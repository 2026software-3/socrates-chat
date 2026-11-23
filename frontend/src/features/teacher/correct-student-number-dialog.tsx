import { useRef, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import { Alert, AlertDescription } from '@/components/ui/alert'
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { StudentRow } from '@/lib/types'

export function CorrectStudentNumberDialog({ student, onClose, onCorrected }: {
  student: StudentRow
  onClose: () => void
  onCorrected: () => void
}) {
  const t = useT()
  const [number, setNumber] = useState(student.student_number)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string>()
  const busy = useRef(false)

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (busy.current || !student.id || number.toUpperCase() === student.student_number) return
    if (!/^[0-9]{8}[a-z]$/i.test(number)) {
      setError(t('error.invalid_student_number'))
      return
    }
    busy.current = true
    setPending(true)
    setError(undefined)
    try {
      await api.put(`/api/students/${encodeURIComponent(student.id)}/student-number`, { student_number: number })
      toast.success(t('teacher.roster.correctedToast'))
      onCorrected()
      onClose()
    } catch (err) {
      setError(errorText(t, err))
    } finally {
      busy.current = false
      setPending(false)
    }
  }

  return (
    <AlertDialog open onOpenChange={(open) => {
      if (!open && !busy.current) onClose()
    }}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('teacher.roster.correctTitle')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('teacher.roster.correctBody', { who: student.display_name ?? student.student_number })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <form onSubmit={submit} noValidate className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="correct-student-number">{t('teacher.roster.newNumber')}</Label>
            <Input
              id="correct-student-number"
              value={number}
              onChange={(e) => setNumber(e.target.value)}
              disabled={pending}
              autoComplete="off"
              spellCheck={false}
              aria-invalid={!!error}
              aria-describedby={error ? 'correct-student-number-error' : undefined}
            />
          </div>
          {error ? (
            <Alert id="correct-student-number-error" variant="destructive" role="alert">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={pending}>{t('common.cancel')}</AlertDialogCancel>
            <Button type="submit" disabled={pending || number.toUpperCase() === student.student_number}>
              {pending ? t('teacher.working') : t('common.save')}
            </Button>
          </AlertDialogFooter>
        </form>
      </AlertDialogContent>
    </AlertDialog>
  )
}
