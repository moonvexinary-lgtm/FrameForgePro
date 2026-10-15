/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "test_utils.hpp"

#include "frameforge/video/alphaglow.h"
#include "frameforge/video/cineon.h"
#include "frameforge/video/edgefeather.h"
#include "frameforge/video/extract.h"
#include "frameforge/video/fourcolorgradient.h"
#include "frameforge/video/lensflare.h"
#include "frameforge/video/levels.h"
#include "frameforge/video/lightning.h"
#include "frameforge/video/magnify.h"
#include "frameforge/video/replicate.h"
#include "frameforge/video/roughenedges.h"
#include "frameforge/video/spherize.h"
#include "frameforge/video/twirl.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <string>
#include <vector>

TEST_CASE("Levels video DSP", "[FrameForge][Video]")
{
    SECTION("Metadata")
    {
        FrameForge::Video::Levels effect;
        REQUIRE(effect.id() == "frameforge.levels");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 5);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
            REQUIRE(param.maximum >= param.minimum);
            REQUIRE(param.defaultValue >= param.minimum);
            REQUIRE(param.defaultValue <= param.maximum);
        }
        REQUIRE(names == "input_black;input_white;gamma;output_black;output_white;");
        REQUIRE(effect.parameter("no_such_parameter") == 0.0);
    }

    SECTION("Identity at defaults")
    {
        FrameForge::Video::Levels effect;
        // A 1x1 frame is enough to verify the transform through the public
        // interface; identity means every 8-bit value maps to itself.
        effect.prepare(1, 1);
        for (int v = 0; v < 256; ++v) {
            uint8_t frame[4] = {static_cast<uint8_t>(v), static_cast<uint8_t>(v), static_cast<uint8_t>(v), 200};
            effect.processFrame(frame, 4, 1, 1);
            REQUIRE(frame[0] == v);
            REQUIRE(frame[1] == v);
            REQUIRE(frame[2] == v);
            REQUIRE(frame[3] == 200); // alpha untouched
        }
    }

    SECTION("Documented transform formula")
    {
        FrameForge::Video::Levels effect;
        effect.setParameter("input_black", 64.0);
        effect.setParameter("input_white", 192.0);
        effect.setParameter("gamma", 2.0);
        effect.setParameter("output_black", 16.0);
        effect.setParameter("output_white", 240.0);
        effect.prepare(4, 1);

        // Build a 4-pixel gradient covering the interesting input values.
        const uint8_t inputs[4] = {0, 64, 128, 255};
        std::vector<uint8_t> frame(4 * 4, 0);
        for (int i = 0; i < 4; ++i) {
            frame[i * 4 + 0] = inputs[i];
            frame[i * 4 + 1] = inputs[i];
            frame[i * 4 + 2] = inputs[i];
            frame[i * 4 + 3] = 255;
        }
        effect.processFrame(frame.data(), 4 * 4, 4, 1);

        for (int i = 0; i < 4; ++i) {
            const double t = std::clamp((static_cast<double>(inputs[i]) - 64.0) / (192.0 - 64.0), 0.0, 1.0);
            const double expected = 16.0 + std::pow(t, 1.0 / 2.0) * (240.0 - 16.0);
            const double got = static_cast<double>(frame[i * 4 + 0]);
            // LUT rounding: within one component step.
            REQUIRE(std::abs(got - expected) <= 1.0);
            // All channels receive the identical transform.
            REQUIRE(frame[i * 4 + 0] == frame[i * 4 + 1]);
            REQUIRE(frame[i * 4 + 1] == frame[i * 4 + 2]);
        }
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Video::Levels effect;
        effect.setParameter("gamma", 100.0);  // above the 10.0 maximum
        REQUIRE(effect.parameter("gamma") == 10.0);
        effect.setParameter("input_white", 400.0); // above the 255 maximum
        REQUIRE(effect.parameter("input_white") == 255.0);
    }

    SECTION("Degenerate input range thresholds")
    {
        FrameForge::Video::Levels effect;
        effect.setParameter("input_black", 128.0);
        effect.setParameter("input_white", 128.0); // zero-width input range
        effect.prepare(1, 1);
        uint8_t below[4] = {10, 10, 10, 255};
        uint8_t above[4] = {200, 200, 200, 255};
        effect.processFrame(below, 4, 1, 1);
        effect.processFrame(above, 4, 1, 1);
        REQUIRE(below[0] == 0);  // below the threshold -> output black
        REQUIRE(above[0] == 255); // above the threshold -> output white
    }

    SECTION("Stride handling")
    {
        FrameForge::Video::Levels effect;
        effect.setParameter("input_white", 128.0); // harsh contrast curve
        effect.prepare(2, 2);
        // 2x2 frame with a padded stride of 12 bytes per row.
        std::vector<uint8_t> frame(12 * 2, 0);
        frame[0 * 12 + 0] = 10;  // row 0, pixel 0: dark
        frame[0 * 12 + 4] = 200; // row 0, pixel 1: bright
        frame[1 * 12 + 0] = 10;  // row 1, pixel 0: dark
        frame[1 * 12 + 4] = 200; // row 1, pixel 1: bright
        effect.processFrame(frame.data(), 12, 2, 2);
        REQUIRE(frame[0] == 0);       // row 0 pixel 0 dark
        REQUIRE(frame[4] == 255);     // row 0 pixel 1 bright
        REQUIRE(frame[12] == 0);      // row 1 pixel 0 dark
        REQUIRE(frame[12 + 4] == 255); // row 1 pixel 1 bright
    }
}

TEST_CASE("Extract video DSP", "[FrameForge][Video]")
{
    SECTION("Metadata")
    {
        FrameForge::Video::Extract effect;
        REQUIRE(effect.id() == "frameforge.extract");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 4);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "black_input;white_input;softness;invert;");
        REQUIRE(effect.parameter("no_such_parameter") == 0.0);
    }

    SECTION("Documented luminance window")
    {
        // Adobe (archived docs): pixels below Black Input Level or above
        // White Input Level appear black; in between -> gray/white.
        FrameForge::Video::Extract effect;
        effect.setParameter("black_input", 64.0);
        effect.setParameter("white_input", 192.0);
        effect.prepare(3, 1);
        uint8_t frame[3 * 4] = {
            32, 32, 32, 255,    // below black level -> black
            128, 128, 128, 200, // between -> proportional gray (~127.5)
            224, 224, 224, 128  // above white level -> black
        };
        effect.processFrame(frame, 12, 3, 1);
        REQUIRE(frame[0] == 0);
        REQUIRE(frame[1] == 0);
        REQUIRE(frame[2] == 0);
        REQUIRE(frame[3] == 255); // alpha untouched
        REQUIRE(std::abs(static_cast<int>(frame[4]) - 127) <= 1);
        REQUIRE(frame[4] == frame[5]);
        REQUIRE(frame[5] == frame[6]); // grayscale output
        REQUIRE(frame[7] == 200);      // alpha untouched
        REQUIRE(frame[8] == 0);
        REQUIRE(frame[11] == 128); // alpha untouched
    }

    SECTION("Invert swaps the result")
    {
        FrameForge::Video::Extract effect;
        effect.setParameter("black_input", 64.0);
        effect.setParameter("white_input", 192.0);
        effect.setParameter("invert", 1.0);
        effect.prepare(2, 1);
        uint8_t frame[2 * 4] = {
            32, 32, 32, 255,   // outside window -> black -> inverted: white
            224, 224, 224, 255 // outside window -> black -> inverted: white
        };
        effect.processFrame(frame, 8, 2, 1);
        REQUIRE(frame[0] == 255);
        REQUIRE(frame[4] == 255);
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Video::Extract effect;
        effect.setParameter("white_input", 999.0); // above the 255 maximum
        REQUIRE(effect.parameter("white_input") == 255.0);
    }
}

TEST_CASE("Edge Feather video DSP", "[FrameForge][Video]")
{
    SECTION("Metadata")
    {
        FrameForge::Video::EdgeFeather effect;
        REQUIRE(effect.id() == "frameforge.edgefeather");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 1);
        REQUIRE(params[0].name == "amount");
        REQUIRE(effect.parameter("no_such_parameter") == 0.0);
    }

    SECTION("Bypass at zero amount")
    {
        FrameForge::Video::EdgeFeather effect;
        effect.prepare(2, 1);
        uint8_t frame[2 * 4] = {255, 10, 20, 255, 1, 2, 3, 0};
        effect.processFrame(frame, 8, 2, 1);
        REQUIRE(frame[3] == 255);
        REQUIRE(frame[7] == 0);
    }

    SECTION("Horizontal edge ramp, RGB untouched")
    {
        // 64x1: alpha 255 for x < 32, 0 after; feather amount 4 -> radius 2.
        FrameForge::Video::EdgeFeather effect;
        effect.setParameter("amount", 4.0);
        effect.prepare(64, 1);
        std::vector<uint8_t> frame(64 * 4, 0);
        for (int x = 0; x < 64; ++x) {
            frame[x * 4 + 0] = 100;
            frame[x * 4 + 1] = 150;
            frame[x * 4 + 2] = 200;
            frame[x * 4 + 3] = x < 32 ? 255 : 0;
        }
        effect.processFrame(frame.data(), 64 * 4, 64, 1);
        // Deep interior stays opaque; far outside stays transparent.
        REQUIRE(frame[10 * 4 + 3] == 255);
        REQUIRE(frame[50 * 4 + 3] == 0);
        // The edge becomes a monotonic non-increasing ramp around the cut.
        for (int x = 30; x < 36; ++x) {
            REQUIRE(frame[x * 4 + 3] >= frame[(x + 1) * 4 + 3]);
        }
        // Intermediate alpha exists right of the cut.
        REQUIRE(frame[32 * 4 + 3] > 0);
        REQUIRE(frame[32 * 4 + 3] < 255);
        // RGB untouched everywhere.
        for (int x = 0; x < 64; ++x) {
            REQUIRE(frame[x * 4 + 0] == 100);
            REQUIRE(frame[x * 4 + 1] == 150);
            REQUIRE(frame[x * 4 + 2] == 200);
        }
    }

    SECTION("Vertical pass on a horizontal boundary")
    {
        // 8x8: alpha 255 for y < 4, 0 after; exercises the vertical pass.
        FrameForge::Video::EdgeFeather effect;
        effect.setParameter("amount", 4.0);
        effect.prepare(8, 8);
        std::vector<uint8_t> frame(8 * 8 * 4, 0);
        for (int y = 0; y < 8; ++y) {
            for (int x = 0; x < 8; ++x) {
                frame[(y * 8 + x) * 4 + 3] = y < 4 ? 255 : 0;
            }
        }
        effect.processFrame(frame.data(), 8 * 4, 8, 8);
        REQUIRE(frame[(0 * 8 + 0) * 4 + 3] == 255); // top row fully opaque
        REQUIRE(frame[(7 * 8 + 0) * 4 + 3] == 0);   // bottom row fully clear
        // Monotonic falloff down the column.
        for (int y = 0; y < 7; ++y) {
            REQUIRE(frame[(y * 8) * 4 + 3] >= frame[((y + 1) * 8) * 4 + 3]);
        }
        // Intermediate alpha on both sides of the boundary.
        const uint8_t above = frame[(3 * 8) * 4 + 3];
        const uint8_t below = frame[(4 * 8) * 4 + 3];
        REQUIRE(above > 0);
        REQUIRE(below > 0);
        REQUIRE(below < 255);
    }
}

TEST_CASE("Alpha Glow video DSP", "[FrameForge][Video]")
{
    // Common fixture: 64x1 frame, opaque alpha for x < 32, transparent after,
    // black RGB everywhere. Glow radius 4 -> window 9 px.
    const auto makeFrame = []() {
        std::vector<uint8_t> frame(64 * 4, 0);
        for (int x = 0; x < 64; ++x) {
            frame[x * 4 + 3] = x < 32 ? 255 : 0;
        }
        return frame;
    };

    SECTION("Metadata")
    {
        FrameForge::Video::AlphaGlow effect;
        REQUIRE(effect.id() == "frameforge.alphaglow");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 4);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "glow;brightness;color_a;color_b;");
        REQUIRE(effect.parameter("no_such_parameter") == 0.0);
    }

    SECTION("Glow outside the alpha, interior untouched")
    {
        FrameForge::Video::AlphaGlow effect;
        effect.setParameter("glow", 4.0);
        effect.prepare(64, 1);
        auto frame = makeFrame();
        effect.processFrame(frame.data(), 64 * 4, 64, 1);

        // Interior (x=31): glow mask <= 0 -> untouched.
        REQUIRE(frame[31 * 4 + 3] == 255);
        REQUIRE(frame[31 * 4 + 0] == 0);
        // Just outside the edge (x=32,33): glow appears (prefix-sum math:
        // 4/9 and 3/9 of 255 -> ~113 and 85 alpha) with the white glow color.
        REQUIRE(frame[32 * 4 + 3] > frame[34 * 4 + 3]); // falloff away from edge
        REQUIRE(std::abs(static_cast<int>(frame[33 * 4 + 3]) - 85) <= 1);
        REQUIRE(frame[33 * 4 + 3] > 0);
        REQUIRE(frame[33 * 4 + 3] < 255);
        REQUIRE(frame[33 * 4 + 0] == 255); // white glow (default colors)
        REQUIRE(frame[33 * 4 + 1] == 255);
        REQUIRE(frame[33 * 4 + 2] == 255);
        // Beyond the spread (x=40): untouched.
        REQUIRE(frame[40 * 4 + 3] == 0);
        REQUIRE(frame[40 * 4 + 0] == 0);
        // Overall falloff.
        REQUIRE(frame[32 * 4 + 3] > frame[35 * 4 + 3]);
    }

    SECTION("Color shift from edge to outer glow")
    {
        FrameForge::Video::AlphaGlow effect;
        effect.setParameter("glow", 4.0);
        effect.setParameter("color_a", 0xFF0000); // red near the edge
        effect.setParameter("color_b", 0x0000FF); // blue at the outer end
        effect.prepare(64, 1);
        auto frame = makeFrame();
        effect.processFrame(frame.data(), 64 * 4, 64, 1);

        // Near the edge (x=32): mostly color_a (red, ~113/255).
        REQUIRE(std::abs(static_cast<int>(frame[32 * 4 + 0]) - 113) <= 1);
        // Farther out (x=35): shifted toward color_b (blue dominates).
        REQUIRE(frame[32 * 4 + 0] > frame[35 * 4 + 0]); // red falls off
        REQUIRE(frame[32 * 4 + 2] < frame[35 * 4 + 2]); // blue grows
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Video::AlphaGlow effect;
        effect.setParameter("glow", 500.0); // above the 100 maximum
        REQUIRE(effect.parameter("glow") == 100.0);
        effect.setParameter("color_a", 20000000.0); // above 0xFFFFFF
        REQUIRE(effect.parameter("color_a") == 16777215.0);
    }
}

TEST_CASE("Twirl video DSP", "[FrameForge][Video]")
{
    const auto makeSplit = [](int size) {
        // Left half black, right half white, full alpha.
        std::vector<uint8_t> frame(static_cast<size_t>(size) * size * 4, 0);
        for (int i = 0; i < size * size; ++i) {
            const uint8_t value = (i % size) < size / 2 ? 0 : 255;
            frame[i * 4 + 0] = value;
            frame[i * 4 + 1] = value;
            frame[i * 4 + 2] = value;
            frame[i * 4 + 3] = 255;
        }
        return frame;
    };

    SECTION("Metadata")
    {
        FrameForge::Video::Twirl effect;
        REQUIRE(effect.id() == "frameforge.twirl");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 2);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "angle;radius;");
    }

    SECTION("Center-dominant rotation (documented)")
    {
        // Adobe: "distorted more sharply in its center than at the edges".
        FrameForge::Video::Twirl effect;
        effect.setParameter("angle", 180.0);
        effect.setParameter("radius", 30.0);
        effect.prepare(64, 64);
        auto frame = makeSplit(64);
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        // 180 degrees at the center: dest(cx+2, cy) samples src(~cx-2, cy) -> black.
        REQUIRE(frame[(32 * 64 + 34) * 4 + 0] < 60);
        // And dest(cx-2, cy) samples src(~cx+2, cy) -> white.
        REQUIRE(frame[(32 * 64 + 30) * 4 + 0] > 200);
        // Outside the radius: untouched.
        REQUIRE(frame[(2 * 64 + 2) * 4 + 0] == 0);
        REQUIRE(frame[(2 * 64 + 60) * 4 + 0] == 255);
    }

    SECTION("Identity at zero angle")
    {
        FrameForge::Video::Twirl effect;
        effect.setParameter("angle", 0.0);
        effect.prepare(64, 64);
        auto frame = makeSplit(64);
        const auto original = frame;
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        REQUIRE(frame == original);
    }
}

TEST_CASE("Spherize video DSP", "[FrameForge][Video]")
{
    const auto makeSplit = [](int size) {
        std::vector<uint8_t> frame(static_cast<size_t>(size) * size * 4, 0);
        for (int i = 0; i < size * size; ++i) {
            const uint8_t value = (i % size) < size / 2 ? 0 : 255;
            frame[i * 4 + 0] = value;
            frame[i * 4 + 1] = value;
            frame[i * 4 + 2] = value;
            frame[i * 4 + 3] = 255;
        }
        return frame;
    };

    SECTION("Metadata")
    {
        FrameForge::Video::Spherize effect;
        REQUIRE(effect.id() == "frameforge.spherize");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 2);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "amount;radius_percent;");
    }

    SECTION("Center bulge blends the central boundary")
    {
        FrameForge::Video::Spherize effect;
        effect.setParameter("amount", 100.0);
        effect.setParameter("radius_percent", 100.0);
        effect.prepare(64, 64);
        auto frame = makeSplit(64);
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        // dest(cx-1, cy): samples ~cx-0.5 -> gray blend of black and white.
        const int blended = frame[(32 * 64 + 31) * 4 + 0];
        REQUIRE(blended > 60);
        REQUIRE(blended < 200);
        // dest(cx-4, cy): samples ~cx-2 -> still black.
        REQUIRE(frame[(32 * 64 + 28) * 4 + 0] < 20);
        // The center and the far corner are untouched.
        REQUIRE(frame[(32 * 64 + 32) * 4 + 0] == 255);
        REQUIRE(frame[(2 * 64 + 2) * 4 + 0] == 0);
        // Alpha preserved by the sampler.
        REQUIRE(frame[(32 * 64 + 31) * 4 + 3] == 255);
    }

    SECTION("Identity at zero amount")
    {
        FrameForge::Video::Spherize effect;
        effect.setParameter("amount", 0.0);
        effect.prepare(64, 64);
        auto frame = makeSplit(64);
        const auto original = frame;
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        REQUIRE(frame == original);
    }
}

TEST_CASE("Cineon Converter video DSP", "[FrameForge][Video]")
{
    const auto onePixel = [](FrameForge::Video::CineonConverter &effect, uint8_t value) {
        uint8_t frame[4] = {value, value, value, 200};
        effect.prepare(1, 1);
        effect.processFrame(frame, 4, 1, 1);
        return frame[0];
    };

    SECTION("Metadata")
    {
        FrameForge::Video::CineonConverter effect;
        REQUIRE(effect.id() == "frameforge.cineonconverter");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 4);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "conversion;black_point;white_point;gamma;");
    }

    SECTION("Documented Cineon anchors (Log -> Linear)")
    {
        FrameForge::Video::CineonConverter effect; // default: Log -> Linear
        // 18% gray (code ~470 -> v8 117) -> linear ~0.19 -> byte ~49.
        REQUIRE(std::abs(static_cast<int>(onePixel(effect, 117)) - 49) <= 2);
        // Reference white (code 685 -> v8 171) -> linear 1.0 -> 255.
        REQUIRE(onePixel(effect, 171) == 255);
        // The toe keeps low codes near black.
        REQUIRE(onePixel(effect, 24) < 8);
        REQUIRE(onePixel(effect, 0) == 0);
        // Alpha passes through.
        uint8_t frame[4] = {117, 117, 117, 200};
        effect.prepare(1, 1);
        effect.processFrame(frame, 4, 1, 1);
        REQUIRE(frame[3] == 200);
    }

    SECTION("Linear -> Cineon Log")
    {
        FrameForge::Video::CineonConverter effect;
        effect.setParameter("conversion", 0.0);
        // Linear 1.0 encodes to code 685 -> byte ~171.
        REQUIRE(std::abs(static_cast<int>(onePixel(effect, 255)) - 171) <= 1);
        REQUIRE(onePixel(effect, 0) == 0);
    }

    SECTION("Round trip within the reference range")
    {
        // Log->Linear then Linear->Log recovers codes <= 685 (v8 <= 171);
        // values above clip to linear 1.0 by design (broadcast-safe range).
        FrameForge::Video::CineonConverter logToLinear; // default
        FrameForge::Video::CineonConverter linearToLog;
        linearToLog.setParameter("conversion", 0.0);
        for (const uint8_t v : {uint8_t(0), uint8_t(24), uint8_t(64), uint8_t(117), uint8_t(171)}) {
            const uint8_t linear = onePixel(logToLinear, v);
            const uint8_t back = onePixel(linearToLog, linear);
            REQUIRE(std::abs(static_cast<int>(back) - static_cast<int>(v)) <= 2);
        }
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Video::CineonConverter effect;
        effect.setParameter("white_point", 5000.0); // above the 1023 maximum
        REQUIRE(effect.parameter("white_point") == 1023.0);
    }
}

TEST_CASE("Magnify video DSP", "[FrameForge][Video]")
{
    const auto makeSplit = [](int size) {
        // Left half black, right half white, full alpha.
        std::vector<uint8_t> frame(static_cast<size_t>(size) * size * 4, 0);
        for (int i = 0; i < size * size; ++i) {
            const uint8_t value = (i % size) < size / 2 ? 0 : 255;
            frame[i * 4 + 0] = value;
            frame[i * 4 + 1] = value;
            frame[i * 4 + 2] = value;
            frame[i * 4 + 3] = 255;
        }
        return frame;
    };

    SECTION("Metadata")
    {
        FrameForge::Video::Magnify effect;
        REQUIRE(effect.id() == "frameforge.magnify");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 4);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "center_x;center_y;radius;magnification;");
    }

    SECTION("Magnifier region samples toward the center")
    {
        FrameForge::Video::Magnify effect;
        effect.setParameter("radius", 16.0);
        effect.setParameter("magnification", 200.0);
        effect.prepare(64, 64);
        auto frame = makeSplit(64);
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        // Inside the circle at 2x: dest(cx-2.5, cy+0.5) samples src
        // (cx-1.25, cy+0.25) = (30.25, 31.75) -> left half -> black.
        REQUIRE(frame[(32 * 64 + 29) * 4 + 0] == 0);
        // dest(cx+2.5, cy+0.5) samples (32.75, 31.75) -> right half -> white.
        REQUIRE(frame[(32 * 64 + 34) * 4 + 0] == 255);
        // Far outside the circle: untouched.
        REQUIRE(frame[(2 * 64 + 2) * 4 + 0] == 0);
        REQUIRE(frame[(2 * 64 + 2) * 4 + 3] == 255);
    }

    SECTION("Identity at 100% magnification")
    {
        FrameForge::Video::Magnify effect;
        effect.setParameter("magnification", 100.0);
        effect.prepare(64, 64);
        auto frame = makeSplit(64);
        const auto original = frame;
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        REQUIRE(frame == original);
    }
}

TEST_CASE("Replicate video DSP", "[FrameForge][Video]")
{
    const auto makeGradient = []() {
        // 4x4 frame, distinct grayscale value v = x + 16*y per pixel.
        std::vector<uint8_t> frame(4 * 4 * 4, 0);
        for (int y = 0; y < 4; ++y) {
            for (int x = 0; x < 4; ++x) {
                const uint8_t v = static_cast<uint8_t>(x + 16 * y);
                frame[(y * 4 + x) * 4 + 0] = v;
                frame[(y * 4 + x) * 4 + 1] = v;
                frame[(y * 4 + x) * 4 + 2] = v;
                frame[(y * 4 + x) * 4 + 3] = 255;
            }
        }
        return frame;
    };

    SECTION("Metadata")
    {
        FrameForge::Video::Replicate effect;
        REQUIRE(effect.id() == "frameforge.replicate");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 2);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "horizontal;vertical;");
    }

    SECTION("2x2 tiles show the whole image (documented)")
    {
        FrameForge::Video::Replicate effect;
        effect.setParameter("horizontal", 2.0);
        effect.setParameter("vertical", 2.0);
        effect.prepare(4, 4);
        auto frame = makeGradient();
        effect.processFrame(frame.data(), 4 * 4, 4, 4);
        // Cell (0,0) shows the whole 4x4 frame downsampled to 2x2:
        // dest local (0,0) -> src (0,0) = 0; local (1,0) -> src (3,0) = 3.
        REQUIRE(frame[(0 * 4 + 0) * 4 + 0] == 0);
        REQUIRE(frame[(0 * 4 + 1) * 4 + 0] == 3);
        // Cell (1,0): dest (2,0) local (0,0) -> src (0,0) = 0 again.
        REQUIRE(frame[(0 * 4 + 2) * 4 + 0] == 0);
        REQUIRE(frame[(0 * 4 + 3) * 4 + 0] == 3);
        // Cell (1,1): dest (1,3) local (1,1) -> src (3,3) = 51.
        REQUIRE(frame[(3 * 4 + 1) * 4 + 0] == 51);
        REQUIRE(frame[(2 * 4 + 2) * 4 + 0] == 0);
        // Alpha is replicated with the tiles.
        REQUIRE(frame[(3 * 4 + 1) * 4 + 3] == 255);
    }

    SECTION("Identity at 1x1 tiles")
    {
        FrameForge::Video::Replicate effect;
        effect.setParameter("horizontal", 1.0);
        effect.setParameter("vertical", 1.0);
        effect.prepare(4, 4);
        auto frame = makeGradient();
        const auto original = frame;
        effect.processFrame(frame.data(), 4 * 4, 4, 4);
        REQUIRE(frame == original);
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Video::Replicate effect;
        effect.setParameter("horizontal", 99.0); // above the 16 maximum
        REQUIRE(effect.parameter("horizontal") == 16.0);
    }
}

TEST_CASE("Four-Color Gradient video DSP", "[FrameForge][Video]")
{
    SECTION("Metadata")
    {
        FrameForge::Video::FourColorGradient effect;
        REQUIRE(effect.id() == "frameforge.fourcolorgradient");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 12);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "x1;y1;x2;y2;x3;y3;x4;y4;color1;color2;color3;color4;");
        REQUIRE(effect.parameter("no_such_parameter") == 0.0);
    }

    SECTION("Solid-color circles at the effect points (documented)")
    {
        // Adobe: "blending them into solid-color circles, each with an
        // effect point as its center" — the point dominates the
        // inverse-square blend, so its color appears at the point.
        FrameForge::Video::FourColorGradient effect;
        effect.prepare(8, 8);
        std::vector<uint8_t> frame(8 * 8 * 4, 0);
        effect.processFrame(frame.data(), 8 * 4, 8, 8);
        // Defaults: corners with red (top-left), green (top-right),
        // yellow (bottom-left), blue (bottom-right).
        const auto px = [&](int x, int y) { return &frame[(y * 8 + x) * 4]; };
        REQUIRE(px(0, 0)[0] == 255); // red
        REQUIRE(px(0, 0)[1] == 0);
        REQUIRE(px(0, 0)[2] == 0);
        REQUIRE(px(7, 0)[1] == 255); // green
        REQUIRE(px(7, 0)[0] == 0);
        REQUIRE(px(0, 7)[0] == 255); // yellow
        REQUIRE(px(0, 7)[1] == 255);
        REQUIRE(px(7, 7)[2] == 255); // blue
        REQUIRE(px(7, 7)[0] == 0);
        // The gradient fills the frame.
        REQUIRE(px(3, 3)[3] == 255);
        // Hand-computed blend at (3,3): weights 0.3332/0.2398/0.2398/0.1875
        // -> (146, 122, 48) within rounding.
        REQUIRE(std::abs(static_cast<int>(px(3, 3)[0]) - 146) <= 2);
        REQUIRE(std::abs(static_cast<int>(px(3, 3)[1]) - 122) <= 2);
        REQUIRE(std::abs(static_cast<int>(px(3, 3)[2]) - 48) <= 2);
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Video::FourColorGradient effect;
        effect.setParameter("color1", 20000000.0); // above 0xFFFFFF
        REQUIRE(effect.parameter("color1") == 16777215.0);
    }
}

TEST_CASE("Roughen Edges video DSP", "[FrameForge][Video]")
{
    const auto makeRect = []() {
        // 64x64: alpha 255 inside the rectangle 16..47, else 0; RGB distinct.
        std::vector<uint8_t> frame(64 * 64 * 4, 0);
        for (int y = 0; y < 64; ++y) {
            for (int x = 0; x < 64; ++x) {
                const size_t i = static_cast<size_t>(y * 64 + x) * 4;
                frame[i + 0] = 100;
                frame[i + 1] = 150;
                frame[i + 2] = 200;
                frame[i + 3] = (x >= 16 && x < 48 && y >= 16 && y < 48) ? 255 : 0;
            }
        }
        return frame;
    };

    SECTION("Metadata")
    {
        FrameForge::Video::RoughenEdges effect;
        REQUIRE(effect.id() == "frameforge.roughenedges");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 3);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "border;scale;evolution;");
    }

    SECTION("Interior and far-outside alpha are preserved, RGB untouched")
    {
        FrameForge::Video::RoughenEdges effect;
        effect.setParameter("border", 8.0);
        effect.setParameter("scale", 16.0);
        effect.prepare(64, 64);
        auto frame = makeRect();
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        // Deep interior: displacement +/-8 px keeps all samples inside the
        // rectangle -> alpha stays 255.
        REQUIRE(frame[(32 * 64 + 32) * 4 + 3] == 255);
        // Far corner: displacement +/-8 px keeps all samples outside
        // (max coordinate 10 < 16) -> alpha stays 0.
        REQUIRE(frame[(2 * 64 + 2) * 4 + 3] == 0);
        // RGB passes through untouched everywhere.
        for (const int x : {2, 20, 32, 45}) {
            const size_t i = static_cast<size_t>(32 * 64 + x) * 4;
            REQUIRE(frame[i + 0] == 100);
            REQUIRE(frame[i + 1] == 150);
            REQUIRE(frame[i + 2] == 200);
        }
    }

    SECTION("Identity at zero border")
    {
        FrameForge::Video::RoughenEdges effect;
        effect.setParameter("border", 0.0);
        effect.prepare(64, 64);
        auto frame = makeRect();
        const auto original = frame;
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        REQUIRE(frame == original);
    }

    SECTION("The edge ring is roughened (deterministic-once-built)")
    {
        // With border 8 / scale 16, the noise displaces samples across the
        // rectangle boundary. These two assertions are deterministic once
        // compiled (fixed hash), but the exact affected pixels cannot be
        // hand-computed; if the hash field proves unlucky at the first M1
        // build, loosen the thresholds (noted in the devlog).
        FrameForge::Video::RoughenEdges effect;
        effect.setParameter("border", 8.0);
        effect.setParameter("scale", 16.0);
        effect.prepare(64, 64);
        auto frame = makeRect();
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        int grown = 0;
        for (int y = 16; y < 48; ++y) {
            if (frame[(y * 64 + 15) * 4 + 3] > 0) {
                ++grown; // alpha grew outward past the left edge
            }
            if (frame[(y * 64 + 16) * 4 + 3] < 255) {
                ++grown; // alpha eroded inward on the left edge
            }
        }
        REQUIRE(grown > 0);
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Video::RoughenEdges effect;
        effect.setParameter("border", 99.0); // above the 32 maximum
        REQUIRE(effect.parameter("border") == 32.0);
    }
}

TEST_CASE("Lens Flare video DSP", "[FrameForge][Video]")
{
    const auto blackFrame = [](int size) {
        std::vector<uint8_t> frame(static_cast<size_t>(size) * size * 4, 0);
        for (int i = 0; i < size * size; ++i) {
            frame[i * 4 + 3] = 255;
        }
        return frame;
    };

    SECTION("Metadata")
    {
        FrameForge::Video::LensFlare effect;
        REQUIRE(effect.id() == "frameforge.lensflare");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 4);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "center_x;center_y;brightness;size;");
    }

    SECTION("Core, ring and ghost are drawn; alpha untouched")
    {
        FrameForge::Video::LensFlare effect;
        effect.setParameter("center_x", 25.0);
        effect.setParameter("center_y", 25.0);
        effect.setParameter("size", 32.0);
        effect.prepare(64, 64);
        auto frame = blackFrame(64);
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        const auto px = [&](int x, int y) { return &frame[(y * 64 + x) * 4]; };
        // Core at the flare center (~15.75, 15.75): strong additive glow.
        REQUIRE(px(16, 16)[0] > 100);
        // Halo ring at 60% of the size (~19.2 px from the center).
        REQUIRE(px(29, 29)[0] > 60);
        REQUIRE(px(29, 29)[0] < 200);
        // Ghost mirrored through the frame center (~47.25, 47.25).
        REQUIRE(px(47, 47)[0] > 40);
        // Far from all elements: untouched black.
        REQUIRE(px(60, 60)[0] == 0);
        // The flare only adds light; alpha passes through.
        REQUIRE(px(16, 16)[3] == 255);
    }

    SECTION("Zero brightness leaves the frame untouched")
    {
        FrameForge::Video::LensFlare effect;
        effect.setParameter("brightness", 0.0);
        effect.prepare(64, 64);
        auto frame = blackFrame(64);
        const auto original = frame;
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        REQUIRE(frame == original);
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Video::LensFlare effect;
        effect.setParameter("brightness", 500.0); // above the 300 maximum
        REQUIRE(effect.parameter("brightness") == 300.0);
    }
}

TEST_CASE("Lightning video DSP", "[FrameForge][Video]")
{
    const auto blackFrame = [](int size) {
        std::vector<uint8_t> frame(static_cast<size_t>(size) * size * 4, 0);
        for (int i = 0; i < size * size; ++i) {
            frame[i * 4 + 3] = 255;
        }
        return frame;
    };

    SECTION("Metadata")
    {
        FrameForge::Video::Lightning effect;
        REQUIRE(effect.id() == "frameforge.lightning");
        REQUIRE(effect.version() == "1.0");
        const auto params = effect.parameters();
        REQUIRE(params.size() == 7);
        std::string names;
        for (const auto &param : params) {
            names += param.name + ";";
        }
        REQUIRE(names == "x1;y1;x2;y2;amplitude;segments;seed;");
    }

    SECTION("Bolt runs between the two specified points")
    {
        FrameForge::Video::Lightning effect;
        effect.setParameter("x1", 0.0);
        effect.setParameter("y1", 0.0);
        effect.setParameter("x2", 90.0);
        effect.setParameter("y2", 90.0);
        effect.setParameter("amplitude", 30.0);
        effect.prepare(64, 64);
        auto frame = blackFrame(64);
        effect.processFrame(frame.data(), 64 * 4, 64, 64);
        const auto px = [&](int x, int y) { return &frame[(y * 64 + x) * 4]; };
        // The endpoints are pinned (the taper forces zero displacement).
        REQUIRE(px(0, 0)[0] == 255);
        REQUIRE(px(57, 57)[0] == 255); // round(0.9 * 63) = 57
        // The bolt is a connected drawn path across the frame.
        int lit = 0;
        for (int i = 0; i < 64 * 64; ++i) {
            if (frame[i * 4] == 255) {
                ++lit;
            }
        }
        REQUIRE(lit >= 40);
        // A point provably farther than the amplitude from the bolt axis
        // (distance |5-60|/sqrt(2) ~ 38.9 > 30) stays untouched.
        REQUIRE(px(5, 60)[0] == 0);
        // Drawing is additive light; alpha passes through.
        REQUIRE(px(0, 0)[3] == 255);
    }

    SECTION("Parameter clamping")
    {
        FrameForge::Video::Lightning effect;
        effect.setParameter("amplitude", 500.0); // above the 200 maximum
        REQUIRE(effect.parameter("amplitude") == 200.0);
    }
}
