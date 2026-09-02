import mergeConfig from './merge.js';

export function build(base, extra) {
  return mergeConfig(base, extra);
}
