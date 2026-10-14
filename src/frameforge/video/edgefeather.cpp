/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/edgefeather.h"

#include "frameforge/video/alphablur.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

EdgeFeather::EdgeFeather() = default;

std::string_view EdgeFeather::id() const
{
    return "frameforge.edgefeather";
}

std::string_view EdgeFeather::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> EdgeFeather::parameters() const
{
    return {
        {"amount", 0.0, 0.0, 100.0}, ///< Border/feather width in pixels
    };
}

void EdgeFeather::setParameter(const std::string &name, double value)
{
    if (name != "amount") {
        // Unknown parameter: ignored per the IVideoEffect contract.
        return;
    }
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            m_amount = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
}

double EdgeFeather::parameter(const std::string &name) const
{
    if (name == "amount") {
        return m_amount;
    }
    // Unknown parameter: return the declared default per the contract.
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            return descriptor.defaultValue;
        }
    }
    return 0.0;
}

void EdgeFeather::prepare(int width, int height)
{
    // Scratch for the separable blur: sized here (allocation is allowed in
    // prepare, never in processFrame).
    m_temp.assign(static_cast<size_t>(std::max(1, width)) * static_cast<size_t>(std::max(1, height)), 0.0f);
    m_line.assign(static_cast<size_t>(std::max(width, height)) + 1, 0.0f);
}

void EdgeFeather::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4 || m_amount < 1.0) {
        return;
    }
    if (m_temp.size() < static_cast<size_t>(width) * static_cast<size_t>(height) || m_line.size() < static_cast<size_t>(std::max(width, height)) + 1) {
        // Host changed geometry without prepare(); refuse rather than
        // corrupt memory (see the IVideoEffect contract).
        return;
    }
    // Total softening width = amount -> radius = ceil(amount / 2).
    const int radius = static_cast<int>(std::ceil(m_amount * 0.5));
    boxBlurAlpha(rgba, stride, width, height, radius, m_temp.data(), m_line.data());
}

} // namespace FrameForge::Video
