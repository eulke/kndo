import { open } from '@demo/lib';
import { connect } from '@demo/lib/client';
import { probe } from '@demo/lib/tools/probe';
import { secret } from '@demo/lib/internal/secret';

export function boot() {
  return open() + connect() + probe() + secret();
}
