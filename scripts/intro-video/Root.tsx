import { INTRO_FRAMES, IntroVideo } from '@/components/intro/IntroVideo';

import { rootFor } from './frame';

/** The whole film, as `render.mjs` renders it without `--scene`. */
export const Root = rootFor(IntroVideo, INTRO_FRAMES);
