# SPDX-FileCopyrightText: 2026 FrameForge Pro contributors
# SPDX-License-Identifier: BSD-2-Clause
# NOTE: edlexport.cpp is temporarily excluded from the build (protected
# member access in TimelineModel needs a friend declaration or public API
# refactor). Re-enable after fixing the access pattern.

set(kdenlive_SRCS
  ${kdenlive_SRCS}
  PARENT_SCOPE
)
