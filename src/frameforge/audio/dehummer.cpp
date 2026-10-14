/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/audio/dehummer.h"

#include <algorithm>
#include <cmath>

namespace
{
constexpr double kPi = 3.14159265358979323846;
}

namespace FrameForge::Audio
{

DeHummer::DeHummer() = default;

std::string_view DeHummer::id() const
{
    return "frameforge.dehummer";
}

std::string_view DeHummer::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> DeHummer::parameters() const
{
    return {
        {"frequency", 60.0, 1.0, 1000.0},   ///< Root frequency of the hum (Hz)
        {"q", 20.0, 0.1, 100.0},            ///< Higher = narrower notches
        {"gain", 30.0, 0.0, 96.0},          ///< Hum attenuation (dB)
        {"harmonics", 3.0, 0.0, 10.0},      ///< Harmonic count above the root
        {"slope", 1.0, 0.0, 1.0},           ///< Per-harmonic attenuation ratio
        {"output_hum_only", 0.0, 0.0, 1.0}, ///< 1 = audition the removed hum
    };
}

void DeHummer::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense DSP).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "frequency") {
        m_frequency = value;
    } else if (name == "q") {
        m_quality = value;
    } else if (name == "gain") {
        m_gain = value;
    } else if (name == "harmonics") {
        m_harmonics = value;
    } else if (name == "slope") {
        m_slope = value;
    } else if (name == "output_hum_only") {
        m_humOnly = value > 0.5;
    } else {
        // Unknown parameter: ignored per the IEffect contract.
        return;
    }
    // Parameter changes happen at block boundaries; rebuild the filter chain
    // and reset the filter states (a fresh convergence of a few milliseconds
    // is preferable to a stale chain).
    rebuildFilters();
    for (auto &states : m_states) {
        states.assign(m_notches.size(), BiquadState{});
    }
}

double DeHummer::parameter(const std::string &name) const
{
    if (name == "frequency") {
        return m_frequency;
    }
    if (name == "q") {
        return m_quality;
    }
    if (name == "gain") {
        return m_gain;
    }
    if (name == "harmonics") {
        return m_harmonics;
    }
    if (name == "slope") {
        return m_slope;
    }
    if (name == "output_hum_only") {
        return m_humOnly ? 1.0 : 0.0;
    }
    // Unknown parameter: return the declared default per the IEffect contract.
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            return descriptor.defaultValue;
        }
    }
    return 0.0;
}

void DeHummer::prepare(double sampleRate, size_t channelCount)
{
    m_sampleRate = std::max(1.0, sampleRate);
    m_channels = channelCount;
    rebuildFilters();
    m_states.assign(channelCount, std::vector<BiquadState>(m_notches.size()));
}

void DeHummer::processPlanar(float *const *channels, size_t channelCount, size_t sampleCount)
{
    // No allocation, no locking: audio thread (see IEffect contract).
    if (channels == nullptr || sampleCount == 0 || m_notches.empty() || m_gain <= 0.0) {
        return;
    }
    const size_t activeChannels = std::min(channelCount, m_states.size());
    for (size_t ch = 0; ch < activeChannels; ++ch) {
        auto &states = m_states[ch];
        if (states.size() < m_notches.size()) {
            // Defensive: host changed the filter chain without prepare();
            // process only what state we have (see IEffect contract).
            continue;
        }
        float *data = channels[ch];
        if (data == nullptr) {
            continue;
        }
        for (size_t i = 0; i < sampleCount; ++i) {
            const double x = data[i];
            double y = x;
            for (size_t n = 0; n < m_notches.size(); ++n) {
                const Notch &c = m_notches[n];
                BiquadState &s = states[n];
                // Direct form I transposed (normalized: a0 == 1).
                const double out = c.b0 * y + s.z1;
                s.z1 = c.b1 * y - c.a1 * out + s.z2;
                s.z2 = c.b2 * y - c.a2 * out;
                y = out;
            }
            data[i] = static_cast<float>(m_humOnly ? (x - y) : y);
        }
    }
}

void DeHummer::rebuildFilters()
{
    m_notches.clear();
    const double nyquist = m_sampleRate * 0.5;
    if (m_gain <= 0.0 || m_frequency < 1.0 || m_frequency >= nyquist) {
        return; // Bypassed (nothing to remove).
    }
    const int harmonicCount = std::max(0, static_cast<int>(m_harmonics));
    const double q = std::max(0.1, m_quality);
    for (int k = 0; k <= harmonicCount; ++k) {
        // k == 0 is the root frequency; k >= 1 are the harmonics above it.
        const double freq = m_frequency * static_cast<double>(k + 1);
        if (freq >= nyquist) {
            break;
        }
        const double attenuationDb = m_gain * std::pow(m_slope, k);
        if (attenuationDb <= 0.05) {
            // Below audibility: stop (slope decays the remaining harmonics).
            break;
        }
        // RBJ audio-EQ-cookbook peaking filter with negative gain: a
        // frequency-selective dip of "attenuationDb" at "freq", flat elsewhere.
        const double w0 = 2.0 * kPi * freq / m_sampleRate;
        const double alpha = std::sin(w0) / (2.0 * q);
        const double a = std::pow(10.0, -attenuationDb / 40.0); // A < 1 for attenuation
        const double cosW0 = std::cos(w0);
        const double a0 = 1.0 + alpha / a;
        Notch notch;
        notch.b0 = (1.0 + alpha * a) / a0;
        notch.b1 = (-2.0 * cosW0) / a0;
        notch.b2 = (1.0 - alpha * a) / a0;
        notch.a1 = (-2.0 * cosW0) / a0;
        notch.a2 = (1.0 - alpha / a) / a0;
        m_notches.push_back(notch);
    }
}

} // namespace FrameForge::Audio
