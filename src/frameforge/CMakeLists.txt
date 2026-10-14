# SPDX-FileCopyrightText: 2026 FrameForge Pro contributors
# SPDX-License-Identifier: BSD-2-Clause

set(kdenlive_SRCS
  ${kdenlive_SRCS}
  frameforge/audio/dehummer.cpp
  frameforge/video/alphablur.cpp
  frameforge/video/alphaglow.cpp
  frameforge/video/cineon.cpp
  frameforge/video/edgefeather.cpp
  frameforge/video/extract.cpp
  frameforge/video/fourcolorgradient.cpp
  frameforge/video/lensflare.cpp
  frameforge/video/levels.cpp
  frameforge/video/lightning.cpp
  frameforge/video/magnify.cpp
  frameforge/video/replicate.cpp
  frameforge/video/roughenedges.cpp
  frameforge/video/spherize.cpp
  frameforge/video/twirl.cpp
  PARENT_SCOPE
)
