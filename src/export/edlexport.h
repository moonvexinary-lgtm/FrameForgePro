/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include <QObject>

#include <memory>

class TimelineItemModel;

/** @brief This class provides support for exporting CMX3600 EDL files.
 *
 * Notes about the EDL export:
 *
 * The CMX3600 format has no concept of stacked video tracks, so the
 * bottom-most video track is exported (the sequence's "V1"). If the timeline
 * has additional video tracks, a comment is written to the file.
 *
 * Events are cuts only ("C"). Same-track mixes/crossfades are absorbed into
 * the adjacent clips' in/out points (the dissolve itself is not represented),
 * which is the standard cuts-only EDL behavior.
 *
 * Timecode is non-drop frame at the project frame rate. Record timecode
 * starts at 00:00:00:00. Source in/out points are expressed in the media's
 * original frame rate when it differs from the project frame rate.
 *
 * Clip names are written as "* FROM CLIP NAME:" comments and media paths as
 * "* FROM FILE:" comments to aid relinking.
 */
class EdlExport : public QObject
{
    Q_OBJECT

public:
    EdlExport(QObject *parent = nullptr);

    /** @brief Export the current timeline as an EDL file. */
    bool exportFile(const QString &fileName);

    /** @brief Export a timeline as an EDL file. */
    bool exportTimeline(const std::shared_ptr<TimelineItemModel> &timeline, const QString &fileName);

public Q_SLOTS:
    /** @brief Open a file dialog and export an EDL file. */
    void slotExport();
};
