export interface Point {
  readonly x: number;
  readonly y: number;
}

/**
 * Tap coordinates in points on the iPhone 18 Pro (402x874), matching ios/snap.sh.
 * GPUI draws inside one UIKit view until gpui-mobile bridges AccessKit to UIKit.
 */
export const POINTS = {
  page: { x: 200, y: 300 },
  'header-done': { x: 364, y: 84 },
  'add-key': { x: 300, y: 84 },
  'fix-grammar': { x: 72, y: 752 },
  'change-teh': { x: 176, y: 182 },
  keep: { x: 106, y: 746 },
  use: { x: 295, y: 746 },
  'accept-rest': { x: 324, y: 93 },
  back: { x: 80, y: 105 },
  'key-field': { x: 200, y: 357 },
  'reveal-key': { x: 355, y: 357 },
  'open-keys': { x: 316, y: 84 },
  'feedback-link': { x: 200, y: 690 },
  'open-feedback': { x: 261, y: 84 },
  'feedback-field': { x: 200, y: 250 },
  'feedback-done': { x: 355, y: 105 },
  'feedback-back': { x: 80, y: 105 },
  'send-feedback': { x: 200, y: 337 },

  // Live editor lanes
  'line-one-end': { x: 300, y: 148 },
  'word-light': { x: 70, y: 148 },
  'word-paragraph': { x: 140, y: 190 },
  'line-two-end': { x: 380, y: 190 },
  'start-handle': { x: 97, y: 170 },

  // Library and reading
  cog: { x: 26, y: 84 },
  'show-writing': { x: 72, y: 84 },
  'show-reading': { x: 116, y: 84 },
  'find-books': { x: 276, y: 140 },
  'found-pdf': { x: 150, y: 341 },
  'add-found': { x: 201, y: 414 },
  'page-forward': { x: 370, y: 450 },
  contents: { x: 305, y: 90 },
  'chapter-one': { x: 120, y: 237 },
  'back-to-library': { x: 192, y: 90 },
  'add-one-found': { x: 200, y: 343 },
  'footnote-marker': { x: 190, y: 344 },
  'reading-middle': { x: 200, y: 650 },
  'page-back': { x: 30, y: 450 },
  'walden-paragraph': { x: 200, y: 300 },
  'pill-highlight': { x: 178, y: 757 },
  'pill-quote': { x: 224, y: 757 },
  'contents-highlights': { x: 297, y: 145 },
  'first-highlight': { x: 200, y: 206 },

  // Appearance sheet
  'sheet-done': { x: 363, y: 402 },
  sync: { x: 353, y: 458 },
  'tone-sepia': { x: 127, y: 582 },
  'pages-scroll': { x: 110, y: 450 },
  'pages-paged': { x: 290, y: 450 },

  // Documents
  'open-documents': { x: 200, y: 84 },
  'new-document': { x: 316, y: 84 },
  'row-one': { x: 200, y: 136 },
  'row-two': { x: 200, y: 182 },
  'undo-remove': { x: 201, y: 810 },

  // Account
  'sheet-account': { x: 150, y: 806 },
  'sign-in': { x: 201, y: 451 },
  'copy-address': { x: 328, y: 201 },
  'try-again': { x: 355, y: 145 },

  // Find
  'open-find': { x: 372, y: 84 },
  'find-done': { x: 367, y: 84 },
  'open-find-tall': { x: 378, y: 90 },
  'find-done-tall': { x: 367, y: 90 },

  // Library delete undo
  'undo-remove-book': { x: 201, y: 810 },
} as const satisfies Record<string, Point>;

export type PointName = keyof typeof POINTS;

export const SHELF_TILES: readonly Point[] = [
  { x: 90, y: 296 },
  { x: 274, y: 296 },
];

export const DELETE_TILE_Y = 249;
