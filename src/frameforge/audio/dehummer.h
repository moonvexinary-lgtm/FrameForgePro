/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include "frameforge/interfaces/ieffect.h"

#include <string>
#include <vector>

namespace FrameForge::Audio
{

/** @brief FrameForge native DeHummer effect.
 *
 *  Implements the Adobe-documented DeHummer behavior (Premiere "DeHummer",
 *  parity matrix section D): removes a root hum frequency and its harmonics
 *  from an audio signal.
 *
 *  Parameters (Adobe-documented semantics):
 *  - "frequency": root frequency of the hum in Hz.
 *  - "q": width of the root frequency and harmonics; higher values affect a
 *    narrower range of frequencies (mapped to biquad Q).
 *  - "gain": amount of hum attenuation in dB.
 *  - "harmonics": how many harmonic frequencies above the root are affected
 *    (0 = root only; 3 = root + 50/100/150-style harmonics).
 *  - "slope": attenuation ratio for harmonic frequencies — each successive
 *    harmonic's attenuation is multiplied by this factor (1.0 = same
 *    attenuation for every harmonic; smaller values attenuate higher
 *    harmonics less).
 *  - "output_hum_only": when 1, outputs the removed hum instead of the
 *    cleaned signal so the removed material can be auditioned.
 *
 *  Implementation: a per-channel cascade of RBJ peaking-EQ biquads with
 *  negative gain (partial notches) at the root and harmonic frequencies,
 *  processed in the time domain (no FFT, no latency).
 */
class DeHummer final : public IEffect
{
public:
    DeHummer();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(double sampleRate, size_t channelCount) override;
    void processPlanar(float *const *channels, size_t channelCount, size_t sampleCount) override;

private:
    struct BiquadState
    {
        double z1{0.0};
        double z2{0.0};
    };
    struct Notch
    {
        // Normalized RBJ peaking-EQ coefficients (a0 == 1).
        double b0{1.0}, b1{0.0}, b2{1.0};
        double a1{0.0}, a2{0.0};
    };

    void rebuildFilters();

    double m_sampleRate{48000.0};
    size_t m_channels{0};

    // Parameters (Adobe-documented semantics, see class documentation).
    double m_frequency{60.0};
    double m_quality{20.0};
    double m_gain{30.0};
    double m_harmonics{3.0};
    double m_slope{1.0};
    bool m_humOnly{false};

    std::vector<Notch> m_notches;
    std::vector<std::vector<BiquadState>> m_states; ///< [channel][notch]
};

} // namespace FrameForge::Audio
