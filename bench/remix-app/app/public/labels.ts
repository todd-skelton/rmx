// Same word lists and generator as bench/rsmix-app/app so both apps render identical rows.
const ADJECTIVES = [
  'pretty', 'large', 'big', 'small', 'tall', 'short', 'long', 'handsome', 'plain', 'quaint',
  'clean', 'elegant', 'easy', 'angry', 'crazy', 'helpful', 'mushy', 'odd', 'unsightly',
  'adorable', 'important', 'inexpensive', 'cheap', 'expensive', 'fancy',
]
const COLOURS = ['red', 'yellow', 'blue', 'green', 'pink', 'brown', 'purple', 'brown', 'white', 'black', 'orange']
const NOUNS = [
  'table', 'chair', 'house', 'bbq', 'desk', 'car', 'pony', 'cookie', 'sandwich', 'burger',
  'pizza', 'mouse', 'keyboard',
]

/** 32-bit LCG matching the Rust next_rand. */
function nextRand(seed: { value: number }, max: number): number {
  seed.value = (Math.imul(seed.value, 1664525) + 1013904223) >>> 0
  return (seed.value >>> 8) % max
}

export function label(seed: { value: number }): string {
  let a = ADJECTIVES[nextRand(seed, 25)]
  let c = COLOURS[nextRand(seed, 11)]
  let n = NOUNS[nextRand(seed, 13)]
  return `${a} ${c} ${n}`
}
