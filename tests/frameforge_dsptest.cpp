/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "test_utils.hpp"

#include "frameforge/audio/dehummer.h"

#include <cmath>
#include <string>
#include <vector>

namespace
{
constexpr double kPi = 3.14159265358979323846;

/** @brief Goertzel single-frequency power measurement (test helper). */
double goertzelPower(const std::vector<float> &signal, double frequency, double sampleRate)
{
    const double coeff = 2.0 * std::cos(2.0 * kPi * frequency / sampleRate);
    double q1 = 0.0, q2 = 0.0;
    for (const float sample : signal) {
        const double q0 = coeff * q1 - q2 + static_cast<double>(sample);
        q2 = q1;
        q1 = q0;
    }
    return q1 * q1 + q2 * q2 - coeff * q1 * q2;
}
} // namespace

TEST_CASE("DeHummer DSP", "[FrameForge][Audio]")
{
    const double fs = 48000.0;
    const size_t sampleCount = 48000; // 1 second

    // Synthetic signal: 50 Hz hum (amplitude 0.5) + 1 kHz content (amplitude 0.2).
    std::vector<float> input(sampleCount);
    for (size_t i = 0; i < sampleCount; ++i) {
        const double t = static_cast<double>(i) / fs;
        input[i] = static_cast<float>(0.5 * std::sin(2.0 * kPi * 50.0 * t) + 0.2 * std::sin(2.0 * kPi * 1000.0 * t));
    }

    SECTION("Metadata")
    {
        FrameForge::Audio::DeHummer effect;
        REQUIRE(effect.id() == "frameforge.dehummer");
        REQUIRE(effect.version() == "1.0");
        // The declared parameters match the Adobe-documented DeHummer controls.
        const auto params = effect.parameters();
        REQUIRE(params.size() == 6);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
            REQUIRE(param.maximum >= param.minimum);
            REQUIRE(param.defaultValue >= param.minimum);
            REQUIRE(param.defaultValue <= param.maximum);
        }
        REQUIRE(names == "frequency;q;gain;harmonics;slope;output_hum_only;");
        // Unknown parameters return the declared defaults (0.0 here).
        REQUIRE(effect.parameter("no_such_parameter") == 0.0);
    }

    SECTION("Removes hum, preserves content")
    {
        FrameForge::Audio::DeHummer effect;
        effect.setParameter("frequency", 50.0);
        effect.prepare(fs, 1);

        std::vector<float> output = input;
        float *channels[1] = {output.data()};
        effect.processPlanar(channels, 1, sampleCount);

        const double humPowerIn = goertzelPower(input, 50.0, fs);
        const double humPowerOut = goertzelPower(output, 50.0, fs);
        const double tonePowerIn = goertzelPower(input, 1000.0, fs);
        const double tonePowerOut = goertzelPower(output, 1000.0, fs);

        // Default gain is 30 dB: expect the hum power reduced by ~30 dB
        // (factor 1e-3); allow margin for the finite-Q transition band.
        REQUIRE(humPowerOut < humPowerIn * 3.0e-3);
        // The 1 kHz content must be preserved (within ~1 dB).
        REQUIRE(tonePowerOut > tonePowerIn * 0.8);
    }

    SECTION("Output hum only")
    {
        FrameForge::Audio::DeHummer effect;
        effect.setParameter("frequency", 50.0);
        effect.setParameter("output_hum_only", 1.0);
        effect.prepare(fs, 1);

        std::vector<float> output = input;
        float *channels[1] = {output.data()};
        effect.processPlanar(channels, 1, sampleCount);

        const double humPower = goertzelPower(output, 50.0, fs);
        const double toneLeakage = goertzelPower(output, 1000.0, fs);
        // The isolated output is dominated by the removed hum, not the content.
        REQUIRE(humPower > toneLeakage * 10.0);
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Audio::DeHummer effect;
        effect.setParameter("frequency", 5000.0);   // above the 1000 Hz maximum
        REQUIRE(effect.parameter("frequency") == 1000.0);
        effect.setParameter("gain", -20.0);         // below the 0 dB minimum
        REQUIRE(effect.parameter("gain") == 0.0);
        // Clamping to gain 0 dB bypasses processing: the signal is untouched.
        effect.prepare(fs, 1);
        std::vector<float> output = input;
        float *channels[1] = {output.data()};
        effect.processPlanar(channels, 1, sampleCount);
        bool untouched = true;
        for (size_t i = 0; i < sampleCount; ++i) {
            if (std::abs(output[i] - input[i]) > 1.0e-6) {
                untouched = false;
                break;
            }
        }
        REQUIRE(untouched);
    }
}
