import '../../src/styles/globals.css';

import { useEffect, useState, type ComponentType } from 'react';
import { Composition, continueRender, delayRender } from 'remotion';

import { INTRO_FPS } from '@/components/intro/scene';
import { introStrings, type IntroStrings } from '@/components/intro/strings';
import { en } from '@/i18n/en';
import type { TranslationKey } from '@/i18n/en';
import { tr } from '@/i18n/tr';

interface Props extends Record<string, unknown> {
  lang: 'tr' | 'en';
  theme: 'dark' | 'light';
  width: number;
  height: number;
}

/**
 * A composition as the app shows it: on the page's colour, in the app's face,
 * under the theme's class -- which in the app sits on <html>.
 */
function framed(Film: ComponentType<{ strings: IntroStrings }>) {
  return function Framed({ lang, theme }: Props) {
    const dictionary: Record<TranslationKey, string> = lang === 'en' ? en : tr;
    // A frame taken before the face has loaded is set in the fallback.
    const [handle] = useState(() => delayRender('InterVariable'));
    useEffect(() => {
      void Promise.all([
        document.fonts.load('400 16px InterVariable'),
        document.fonts.load('600 16px InterVariable'),
      ]).then(() => continueRender(handle));
    }, [handle]);

    return (
      <div
        lang={lang}
        className={`${theme === 'dark' ? 'dark ' : ''}absolute inset-0 overflow-hidden bg-bg font-sans text-fg antialiased`}
      >
        <Film strings={introStrings((key) => dictionary[key])} />
      </div>
    );
  };
}

export function rootFor(Film: ComponentType<{ strings: IntroStrings }>, durationInFrames: number) {
  const Framed = framed(Film);
  return function Root() {
    return (
      <Composition
        id="intro"
        component={Framed}
        fps={INTRO_FPS}
        durationInFrames={durationInFrames}
        width={390}
        height={844}
        defaultProps={{ lang: 'tr', theme: 'dark', width: 390, height: 844 } satisfies Props}
        calculateMetadata={({ props }) => ({ width: props.width, height: props.height })}
      />
    );
  };
}
