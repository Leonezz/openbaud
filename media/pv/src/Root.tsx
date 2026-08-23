import React from 'react';
import {Composition} from 'remotion';
import {OpenBaudPV} from './OpenBaudPV';

export const OpenBaudRoot: React.FC = () => (
  <Composition
    id="OpenBaudPV"
    component={OpenBaudPV}
    durationInFrames={1380}
    fps={30}
    width={1920}
    height={1080}
  />
);
