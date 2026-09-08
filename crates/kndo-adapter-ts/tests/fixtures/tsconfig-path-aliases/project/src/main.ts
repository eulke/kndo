import { format } from '~utils'
import { probe } from '@/nested/probe'

export function main(): string {
  return format(probe())
}
