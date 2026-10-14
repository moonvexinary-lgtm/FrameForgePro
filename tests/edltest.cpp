/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "test_utils.hpp"

#include "export/edlexport.h"
#include "otio/otioimport.h"

#include <QApplication>
#include <QFile>
#include <QRegularExpression>
#include <QTemporaryDir>

using namespace fakeit;

TEST_CASE("Export", "[EDL]")
{
    SECTION("OTIO import then EDL export")
    {
        // Create the test document and timeline.
        auto binModel = pCore->projectItemModel();
        std::shared_ptr<DocUndoStack> undoStack = std::make_shared<DocUndoStack>(nullptr);
        KdenliveDoc document(undoStack);
        pCore->projectManager()->testSetDocument(&document);
        QDateTime documentDate = QDateTime::currentDateTime();
        KdenliveTests::updateTimeline(false, QString(), QString(), documentDate, 0);
        auto timeline = document.getTimeline(document.uuid());
        pCore->projectManager()->testSetActiveTimeline(timeline);

        // Import the OTIO test timeline.
        QString inputPath = sourcesPath + "/dataset/test-tracks-clips.otio";
        OtioImport otioImport;
        otioImport.importFile(inputPath, false);
        while (pCore->currentDoc()->loading) {
            qApp->processEvents();
        }

        // Export the EDL.
        EdlExport edlExport;
        QTemporaryDir dir;
        QString outputPath = dir.filePath("test-tracks-clips.edl");
        REQUIRE(edlExport.exportFile(outputPath));

        // Verify the EDL content.
        QFile file(outputPath);
        REQUIRE(file.open(QIODevice::ReadOnly | QIODevice::Text));
        const QString content = QString::fromUtf8(file.readAll());
        file.close();

        REQUIRE(content.startsWith(QStringLiteral("TITLE: ")));
        REQUIRE(content.contains(QStringLiteral("FCM: NON-DROP FRAME")));

        // At least one cut event: "NNN  REEL  ...  V  C  <timecode> <timecode> <timecode> <timecode>"
        static const QRegularExpression eventRe(QStringLiteral("^\\d{3}  .+V  C  \\d{2}:\\d{2}:\\d{2}:\\d{2} \\d{2}:\\d{2}:\\d{2}:\\d{2} "
                                                                "\\d{2}:\\d{2}:\\d{2}:\\d{2} \\d{2}:\\d{2}:\\d{2}:\\d{2}$"),
                                                QRegularExpression::MultilineOption);
        REQUIRE(content.count(eventRe) >= 1);

        // Clip names are exported for relinking.
        REQUIRE(content.contains(QStringLiteral("* FROM CLIP NAME: ")));
    }
}
