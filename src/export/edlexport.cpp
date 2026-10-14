/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "edlexport.h"

#include "bin/projectclip.h"
#include "bin/projectitemmodel.h"
#include "core.h"
#include "doc/kdenlivedoc.h"
#include "mainwindow.h"
#include "project/projectmanager.h"
#include "timeline2/model/clipmodel.hpp"
#include "timeline2/model/timelineitemmodel.hpp"
#include "utils/uiutils.h"

#include <KLocalizedString>
#include <KMessageBox>

#include <QApplication>
#include <QFile>
#include <QFileInfo>
#include <QTextStream>

#include <algorithm>

namespace
{
/** @brief Format a frame count as a non-drop-frame SMPTE timecode at the given rate. */
QString formatTimecode(int frames, int rate)
{
    if (rate < 1) {
        rate = 1;
    }
    const int ff = frames % rate;
    int totalSeconds = frames / rate;
    const int ss = totalSeconds % 60;
    totalSeconds /= 60;
    const int mm = totalSeconds % 60;
    const int hh = totalSeconds / 60;
    return QStringLiteral("%1:%2:%3:%4")
        .arg(hh, 2, 10, QLatin1Char('0'))
        .arg(mm, 2, 10, QLatin1Char('0'))
        .arg(ss, 2, 10, QLatin1Char('0'))
        .arg(ff, 2, 10, QLatin1Char('0'));
}

/** @brief CMX3600 reel names are limited to 8 characters. Derive a sanitized, uppercase reel
 *  name from the media file name, falling back to "AX" (auxiliary source). */
QString reelFromUrl(const QString &url)
{
    const QString baseName = QFileInfo(url).completeBaseName().toUpper();
    QString reel;
    for (const QChar &c : baseName) {
        if (c.isLetterOrNumber()) {
            reel.append(c);
        }
        if (reel.size() >= 8) {
            break;
        }
    }
    return reel.isEmpty() ? QStringLiteral("AX") : reel;
}

struct EdlClipData
{
    int id;
    int position;
    int in;
    int out;
};
} // namespace

EdlExport::EdlExport(QObject *parent)
    : QObject(parent)
{
}

bool EdlExport::exportFile(const QString &fileName)
{
    return exportTimeline(pCore->currentDoc()->getTimeline(pCore->currentTimelineId()), fileName);
}

void EdlExport::slotExport()
{
    QString fileName = UiUtils::getSaveFileName(pCore->window(), i18n("EDL Export"), pCore->currentDoc()->projectDataFolder(),
                                                QStringLiteral("%1 (*.edl)").arg(i18n("CMX3600 EDL")), QStringLiteral(".edl"));
    if (fileName.isNull()) {
        return;
    }
    // Ensure the file gets a .edl extension
    if (!fileName.endsWith(QStringLiteral(".edl"), Qt::CaseInsensitive)) {
        fileName += QStringLiteral(".edl");
    }
    exportFile(fileName);
}

bool EdlExport::exportTimeline(const std::shared_ptr<TimelineItemModel> &timeline, const QString &fileName)
{
    // CMX3600 has no concept of stacked video tracks; export the bottom-most
    // video track (the sequence's "V1").
    QList<int> videoTrackIds;
    const int tracksCount = timeline->getTracksCount();
    for (int index = 0; index < tracksCount; index++) {
        const int trackId = timeline->getTrackIndexFromPosition(index);
        if (!timeline->isAudioTrack(trackId) && !timeline->isSubtitleTrack(trackId)) {
            videoTrackIds.append(trackId);
        }
    }
    if (videoTrackIds.isEmpty()) {
        if (pCore->window()) {
            KMessageBox::error(qApp->activeWindow(), i18n("The timeline has no video track to export."), i18n("Error exporting EDL file"));
        } else {
            qWarning() << "Error exporting EDL file: no video track found";
        }
        return false;
    }
    const int trackId = videoTrackIds.last();
    std::shared_ptr<TrackModel> track = timeline->getTrackById(trackId);

    // Gather clips, ordered by position. Mixes (same-track crossfades) are
    // absorbed into the adjacent clips' in/out points so the resulting events
    // remain valid cuts — mirroring the OpenTimelineIO export behavior.
    std::vector<EdlClipData> orderedClips;
    for (int i = 0; i < track->getClipsCount(); ++i) {
        const int clipId = track->getClipByRow(i);
        int clipPos = timeline->getClipPosition(clipId);
        std::pair<int, int> clipInOut = timeline->getClipInOut(clipId);
        const auto mixInfo = track->getMixInfo(clipId);
        if (mixInfo.first.firstClipId != -1 && mixInfo.first.secondClipId != -1) {
            auto clipModel = timeline->getClipPtr(clipId);
            const int offset = clipModel->getMixDuration() - clipModel->getMixCutPosition();
            clipPos += offset;
            clipInOut.first += offset;
        }
        if (mixInfo.second.firstClipId != -1 && mixInfo.second.secondClipId != -1) {
            auto clipModel = timeline->getClipPtr(mixInfo.second.secondClipId);
            clipInOut.second -= clipModel->getMixCutPosition();
        }
        orderedClips.push_back({clipId, clipPos, clipInOut.first, clipInOut.second});
    }
    std::sort(orderedClips.begin(), orderedClips.end(), [](const EdlClipData &a, const EdlClipData &b) { return a.position < b.position; });

    // Frame rates: record timecode uses the project rate; source timecode uses
    // the media's original rate when it differs.
    const std::pair<int, int> fpsInfo = pCore->getProjectFpsInfo();
    const double projectFps = fpsInfo.second > 0 ? (fpsInfo.first / static_cast<double>(fpsInfo.second)) : 24.0;
    const int recordRate = qRound(projectFps);

    // Title: the sequence name when there is a single sequence, otherwise the
    // project file name.
    QString title;
    const QStringList sequenceNames = pCore->currentDoc()->getSequenceNames();
    if (sequenceNames.size() == 1) {
        title = sequenceNames.first();
    }
    if (title.isEmpty()) {
        title = QFileInfo(pCore->currentDoc()->url().fileName()).completeBaseName();
    }
    if (title.isEmpty()) {
        title = QStringLiteral("SEQUENCE");
    }

    QFile file(fileName);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) {
        if (pCore->window()) {
            KMessageBox::error(qApp->activeWindow(), i18n("Cannot open file %1", fileName), i18n("Error exporting EDL file"));
        } else {
            qWarning() << "Error exporting EDL file: cannot open" << fileName;
        }
        return false;
    }
    QTextStream stream(&file);
    stream << "TITLE: " << title.toUpper() << "\n";
    stream << "FCM: NON-DROP FRAME\n";
    if (videoTrackIds.size() > 1) {
        stream << "* COMMENT: SEQUENCE HAS " << videoTrackIds.size() << " VIDEO TRACKS; EXPORTING BOTTOM VIDEO TRACK (V1)\n";
    }
    stream << "\n";

    int event = 0;
    for (const auto &clip : orderedClips) {
        if (clip.out < clip.in) {
            // Degenerate clip (fully absorbed by a mix); skip.
            continue;
        }
        ++event;
        const QString clipName = timeline->getClipName(clip.id);
        const QString clipBinId = timeline->getClipBinId(clip.id);
        QString clipUrl;
        double mediaFps = projectFps;
        if (std::shared_ptr<ProjectClip> projectClip = pCore->projectItemModel()->getClipByBinID(clipBinId)) {
            if (projectClip->hasUrl()) {
                clipUrl = projectClip->url();
            }
            const double originalFps = projectClip->getOriginalFps();
            if (originalFps > 0) {
                mediaFps = originalFps;
            }
        }
        const QString reel = reelFromUrl(clipUrl);
        // Express the source in/out points (project frames) in the media's timebase.
        const int srcIn = qRound(clip.in / projectFps * mediaFps);
        const int srcOut = qRound(clip.out / projectFps * mediaFps);
        // Record in/out: inclusive last frame, timeline starting at frame 0.
        const int recIn = clip.position;
        const int recOut = clip.position + (clip.out - clip.in);
        stream << QStringLiteral("%1  %2  V  C  %3 %4 %5 %6\n")
                      .arg(event, 3, 10, QLatin1Char('0'))
                      .arg(reel, -8)
                      .arg(formatTimecode(srcIn, qRound(mediaFps)), formatTimecode(srcOut, qRound(mediaFps)), formatTimecode(recIn, recordRate),
                           formatTimecode(recOut, recordRate));
        if (!clipName.isEmpty()) {
            stream << "* FROM CLIP NAME: " << clipName << "\n";
        }
        if (!clipUrl.isEmpty()) {
            stream << "* FROM FILE: " << clipUrl << "\n";
        }
    }
    file.close();
    return true;
}
