// The Plasma widget for QuantumEngine.
//
// Plasma has no first-class QML binding for an arbitrary D-Bus service, so
// this reaches `quantumengined` (the service) through the "executable" data engine:
// `quantumenginectl --json status` to read state, `quantumenginectl <command>`
// to change it. That is the same service, over the same D-Bus contract, that
// the window and the tray icon use, and `quantumenginectl` starts it if it
// isn't running.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.plasma5support as Plasma5Support

PlasmoidItem {
    id: root

    readonly property string statusCommand: "quantumenginectl --json status"
    readonly property int lowBattery: 15

    // Set in the widget's settings: QuantumEngine violet, the Plasma theme's
    // highlight, or a colour of the user's choosing.
    readonly property color accent: Plasmoid.configuration.accentMode === 1 ? Kirigami.Theme.highlightColor
        : Plasmoid.configuration.accentMode === 2 ? Plasmoid.configuration.accentColor
        : "#a46bff"
    readonly property bool wideLayout: Plasmoid.formFactor === PlasmaCore.Types.Horizontal
    readonly property int firstControlRow: updateVersion !== "" ? 3 : 2

    // Empty until the first successful read, so a failing command never
    // passes for a real "no dongle" answer.
    property string link: ""
    // From QuantumEngine's list of models: the headset being served, or an
    // unverified one that is plugged in. Empty with an older service.
    property string modelName: ""
    property string modelKind: ""
    property bool modelVerified: true
    // A USB-wired model (Quantum 400): no battery or noise control.
    property bool modelWired: false
    // Why the last status read failed, shown in place of the controls.
    property string statusError: ""
    property int battery: 0
    property string anc: "off"
    property bool lightsOn: false

    // The EQ runs on the PC, so it's usable whenever the service answers,
    // headset connected or not. Older services don't report it at all.
    property bool eqSupported: false
    property bool eqEnabled: false
    property string eqProfile: "Flat"
    property var eqProfiles: []
    property string eqStatus: ""

    property string updateVersion: ""
    property string updateUrl: ""
    property bool updateCanInstall: false
    property bool updateBusy: false
    property string updateStatus: ""

    // A `Set*` command is in flight. Controls stay disabled until it answers,
    // so repeated clicks can't pile commands up on the headset.
    property bool busy: false
    property string errorText: ""

    readonly property bool serviceUp: link !== "" && statusError === ""
    readonly property bool ready: link === "ready" && statusError === ""
    readonly property bool batteryLow: ready && !modelWired && battery <= lowBattery

    function statusText() {
        if (statusError !== "") {
            return statusError;
        }
        switch (link) {
        case "ready": return "Connected";
        case "no_dongle": return "No dongle plugged in";
        case "no_access": return "Cannot access the dongle";
        case "unverified":
            if (modelKind !== "" && modelKind !== "headset") {
                return `No headset (${modelName} is a ${modelKind})`;
            }
            return `${modelName !== "" ? modelName : "A headset"}: unverified, open QuantumEngine`;
        case "headset_off": return "Headset off or out of range";
        default: return "Starting…";
        }
    }

    function batteryIcon() {
        if (!ready) {
            return "battery-missing-symbolic";
        }
        const step = Math.round(battery / 10) * 10;
        return `battery-${String(step).padStart(3, "0")}-symbolic`;
    }

    function runSet(command) {
        busy = true;
        executable.connectSource(command);
    }

    function setAnc(mode) {
        if (busy || mode === anc) {
            return;
        }
        anc = mode; // optimistic; the next status read corrects it if it failed
        runSet(`quantumenginectl anc ${mode}`);
    }

    function setLights(on) {
        if (busy) {
            return;
        }
        lightsOn = on;
        runSet(`quantumenginectl lights ${on ? "on" : "off"}`);
    }

    function setEqEnabled(on) {
        if (busy) {
            return;
        }
        eqEnabled = on;
        runSet(`quantumenginectl eq ${on ? "on" : "off"}`);
    }

    function setEqProfile(name) {
        if (busy || name === eqProfile) {
            return;
        }
        eqProfile = name;
        // Profile names are the service's own, but quote them for the shell
        // all the same.
        runSet(`quantumenginectl eq '${name.replace(/'/g, "")}'`);
    }

    // Not `runSet`: this runs until the install finishes, and the headset
    // controls shouldn't lock up meanwhile. Progress arrives in updateStatus.
    function installUpdate() {
        updateBusy = true;
        executable.connectSource("quantumenginectl update install");
    }

    Plasmoid.icon: "audio-headphones-symbolic"
    Plasmoid.status: {
        if (batteryLow) {
            return PlasmaCore.Types.NeedsAttentionStatus;
        }
        return ready ? PlasmaCore.Types.ActiveStatus : PlasmaCore.Types.PassiveStatus;
    }

    toolTipMainText: "QuantumEngine"
    toolTipSubText: {
        const eq = eqSupported ? ` · EQ ${eqEnabled ? eqProfile : "off"}` : "";
        const update = updateVersion !== "" ? `\nUpdate ${updateVersion} available` : "";
        const power = modelWired ? "Wired" : `Battery ${battery}% · ANC ${ancLabel(anc)}`;
        return (ready ? `${power} · Lights ${lightsOn ? "on" : "off"}${eq}` : statusText()) + update;
    }

    function ancLabel(mode) {
        switch (mode) {
        case "on": return "on";
        case "talkthru": return "TalkThru";
        default: return "off";
        }
    }

    Plasma5Support.DataSource {
        id: executable
        engine: "executable"

        onNewData: (sourceName, data) => {
            disconnectSource(sourceName);
            if (sourceName === root.statusCommand) {
                if (data["exit code"] !== 0) {
                    const msg = String(data.stderr).replace(/^quantumenginectl: /, "").trim();
                    root.statusError = msg !== "" ? msg : "quantumenginectl could not be run";
                    return;
                }
                try {
                    const s = JSON.parse(data.stdout);
                    if (s.link === undefined) {
                        // The pre-D-Bus quantumenginectl answers without a
                        // `link` field, straight from the hardware.
                        root.statusError = "The installed quantumenginectl is too old for this widget — install the current QuantumEngine build";
                        return;
                    }
                    root.statusError = "";
                    root.link = s.link;
                    root.modelName = s.model ? s.model.name : "";
                    root.modelKind = s.model ? s.model.kind : "";
                    root.modelVerified = s.model ? s.model.verified : true;
                    root.modelWired = s.model ? s.model.family === "wired" : false;
                    root.battery = s.battery_percent;
                    // Don't let a poll that raced a command overwrite the
                    // optimistic value before that command has answered.
                    if (!root.busy) {
                        root.anc = s.anc;
                        root.lightsOn = s.lights_on;
                    }
                    root.eqSupported = s.eq !== undefined;
                    if (root.eqSupported) {
                        root.eqProfiles = s.eq.profiles;
                        root.eqStatus = s.eq.status;
                        if (!root.busy) {
                            root.eqEnabled = s.eq.enabled;
                            root.eqProfile = s.eq.profile;
                        }
                    }
                    if (s.update !== undefined) {
                        root.updateVersion = s.update.available;
                        root.updateUrl = s.update.url;
                        root.updateCanInstall = s.update.can_install;
                        root.updateBusy = s.update.busy;
                        root.updateStatus = s.update.status;
                    }
                } catch (e) {
                    // A half-written line from a service that's just starting.
                }
                return;
            }
            if (sourceName.startsWith("quantumenginectl ")) {
                root.busy = false;
                root.errorText = data["exit code"] === 0
                    ? ""
                    : String(data.stderr).replace(/^quantumenginectl: /, "").trim();
                root.statusSince = Date.now();
                connectSource(root.statusCommand);
            }
        }
    }

    // When the running status request started. Asking again while it runs
    // does nothing, so one that never finished would leave the widget on
    // "Starting…" for good; after this long it is dropped and asked afresh.
    property double statusSince: 0
    readonly property int statusGiveUpMs: 30000

    Timer {
        interval: 2000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: {
            if (executable.connectedSources.indexOf(root.statusCommand) !== -1) {
                if (Date.now() - root.statusSince < root.statusGiveUpMs) {
                    return;
                }
                executable.disconnectSource(root.statusCommand);
            }
            root.statusSince = Date.now();
            executable.connectSource(root.statusCommand);
        }
    }

    compactRepresentation: MouseArea {
        id: compact

        readonly property bool vertical: Plasmoid.formFactor === PlasmaCore.Types.Vertical

        Layout.minimumWidth: vertical ? -1 : compactRow.implicitWidth
        Layout.preferredWidth: vertical ? -1 : compactRow.implicitWidth
        hoverEnabled: true
        onClicked: root.expanded = !root.expanded

        RowLayout {
            id: compactRow
            anchors.fill: parent
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                Layout.fillHeight: true
                Layout.preferredWidth: height
                source: "audio-headphones-symbolic"
                active: compact.containsMouse
                opacity: root.ready ? 1 : 0.5
            }

            PlasmaComponents3.Label {
                visible: root.ready && !root.modelWired && !compact.vertical
                text: `${root.battery}%`
                color: root.batteryLow ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
            }
        }
    }

    fullRepresentation: GridLayout {
        columns: root.wideLayout ? 2 : 1
        columnSpacing: Kirigami.Units.largeSpacing
        rowSpacing: Kirigami.Units.largeSpacing
        Layout.preferredWidth: Kirigami.Units.gridUnit * (root.wideLayout ? 34 : 19)
        Layout.minimumWidth: Kirigami.Units.gridUnit * (root.wideLayout ? 30 : 16)

        // Header: what this is and whether it's connected.
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            Layout.columnSpan: root.wideLayout ? 2 : 1
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: "audio-headset-symbolic"
                color: root.accent
                Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                Layout.preferredHeight: Kirigami.Units.iconSizes.medium
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                Kirigami.Heading {
                    level: 4
                    text: root.modelName !== "" && root.link === "ready"
                        ? root.modelName + (root.modelVerified ? "" : " (unverified)")
                        : "JBL Quantum 810"
                }
                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    text: root.statusText()
                    opacity: 0.7
                    elide: Text.ElideRight
                }
            }

            PlasmaComponents3.BusyIndicator {
                visible: root.busy
                Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
            }
        }

        Kirigami.Separator {
            Layout.fillWidth: true
            Layout.columnSpan: root.wideLayout ? 2 : 1
        }

        // A newer release, when the service has found one.
        RowLayout {
            visible: root.updateVersion !== ""
            Layout.fillWidth: true
            Layout.columnSpan: root.wideLayout ? 2 : 1
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: "system-software-update-symbolic"
                Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    text: `Update ${root.updateVersion} available`
                    font.bold: true
                    elide: Text.ElideRight
                }
                PlasmaComponents3.Label {
                    visible: root.updateStatus !== ""
                    Layout.fillWidth: true
                    text: root.updateStatus
                    opacity: 0.7
                    wrapMode: Text.WordWrap
                }
            }

            PlasmaComponents3.Button {
                visible: root.updateCanInstall
                enabled: !root.updateBusy
                text: "Install"
                onClicked: root.installUpdate()
            }
            PlasmaComponents3.ToolButton {
                visible: !root.updateCanInstall
                text: "View"
                onClicked: Qt.openUrlExternally(root.updateUrl)
            }
        }

        // Battery.
        RowLayout {
            visible: root.ready && !root.modelWired
            Layout.fillWidth: true
            Layout.column: 0
            Layout.row: root.firstControlRow
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: root.batteryIcon()
                color: root.batteryLow ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                isMask: true
                Layout.preferredWidth: Kirigami.Units.iconSizes.large
                Layout.preferredHeight: Kirigami.Units.iconSizes.large
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                RowLayout {
                    Kirigami.Heading {
                        level: 1
                        text: `${root.battery}%`
                        color: root.batteryLow ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                    }
                    PlasmaComponents3.Label {
                        Layout.alignment: Qt.AlignBottom
                        Layout.bottomMargin: Kirigami.Units.smallSpacing
                        text: root.batteryLow ? "Low battery" : "Battery"
                        opacity: 0.7
                    }
                }
                PlasmaComponents3.ProgressBar {
                    Layout.fillWidth: true
                    from: 0
                    to: 100
                    value: root.battery
                }
            }
        }

        // Noise control: one of three, always exactly one selected.
        ColumnLayout {
            visible: root.ready && !root.modelWired
            Layout.fillWidth: true
            Layout.column: root.wideLayout ? 1 : 0
            Layout.row: root.wideLayout ? root.firstControlRow : root.firstControlRow + 1
            spacing: Kirigami.Units.smallSpacing

            PlasmaComponents3.Label {
                Layout.leftMargin: Kirigami.Units.smallSpacing
                text: "Noise control"
                font.bold: true
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: [
                        { mode: "off", label: "Off", icon: "audio-headphones-symbolic" },
                        { mode: "on", label: "ANC", icon: "audio-volume-muted-symbolic" },
                        { mode: "talkthru", label: "TalkThru", icon: "audio-input-microphone-symbolic" },
                    ]

                    delegate: PlasmaComponents3.ToolButton {
                        required property var modelData

                        Layout.fillWidth: true
                        Layout.preferredWidth: 1 // equal thirds
                        display: QQC2.AbstractButton.TextUnderIcon
                        icon.name: modelData.icon
                        icon.width: Kirigami.Units.iconSizes.medium
                        icon.height: Kirigami.Units.iconSizes.medium
                        text: modelData.label
                        // Not checkable: clicking must not flip `checked` and
                        // break its binding — the headset's state decides it.
                        checked: root.anc === modelData.mode
                        enabled: !root.busy
                        onClicked: root.setAnc(modelData.mode)
                        // A tint of the accent, so the label stays readable on
                        // any Plasma theme.
                        background: Rectangle {
                            radius: 8
                            color: root.anc === modelData.mode ? Qt.alpha(root.accent, 0.28)
                                : parent.hovered ? Qt.alpha(root.accent, 0.12) : "transparent"
                            border.width: root.anc === modelData.mode ? 1 : 0
                            border.color: root.accent
                        }
                    }
                }
            }
        }

        // Lights.
        RowLayout {
            visible: root.ready
            Layout.fillWidth: true
            Layout.column: root.wideLayout ? 1 : 0
            Layout.row: root.firstControlRow + (root.wideLayout ? 1 : 2)
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: root.lightsOn ? "flashlight-on-symbolic" : "flashlight-off-symbolic"
                Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: "Lights"
                font.bold: true
            }

            PlasmaComponents3.Switch {
                checked: root.lightsOn
                enabled: !root.busy
                onToggled: {
                    root.setLights(checked);
                    // Toggling replaced the binding with a fixed value;
                    // restore it so later reads keep the switch in sync.
                    checked = Qt.binding(() => root.lightsOn);
                }
            }
        }

        // Shown instead of the controls while there's nothing to control.
        ColumnLayout {
            visible: !root.ready
            Layout.fillWidth: true
            Layout.column: 0
            Layout.row: root.firstControlRow
            Layout.columnSpan: root.wideLayout ? 2 : 1
            Layout.topMargin: Kirigami.Units.largeSpacing
            Layout.bottomMargin: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                Layout.alignment: Qt.AlignHCenter
                source: "audio-headphones-symbolic"
                opacity: 0.5
                Layout.preferredWidth: Kirigami.Units.iconSizes.huge
                Layout.preferredHeight: Kirigami.Units.iconSizes.huge
            }
            PlasmaComponents3.Label {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                text: root.statusText()
                wrapMode: Text.WordWrap
                opacity: 0.7
            }
        }

        // Equalizer.
        ColumnLayout {
            visible: root.serviceUp && root.eqSupported
            Layout.fillWidth: true
            Layout.column: root.wideLayout ? 1 : 0
            Layout.row: root.firstControlRow + (root.wideLayout ? 2 : 3)
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing

                Kirigami.Icon {
                    source: "view-media-equalizer-symbolic"
                    Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                    Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                }

                PlasmaComponents3.Label {
                    Layout.fillWidth: true
                    text: "Equalizer"
                    font.bold: true
                }

                PlasmaComponents3.Switch {
                    checked: root.eqEnabled
                    enabled: !root.busy
                    onToggled: {
                        root.setEqEnabled(checked);
                        checked = Qt.binding(() => root.eqEnabled);
                    }
                }
            }

            PlasmaComponents3.ComboBox {
                Layout.fillWidth: true
                enabled: root.eqEnabled && !root.busy
                model: root.eqProfiles
                currentIndex: root.eqProfiles.indexOf(root.eqProfile)
                onActivated: index => {
                    root.setEqProfile(root.eqProfiles[index]);
                    currentIndex = Qt.binding(() => root.eqProfiles.indexOf(root.eqProfile));
                }
            }

            PlasmaComponents3.Label {
                visible: root.eqStatus !== ""
                Layout.fillWidth: true
                text: root.eqStatus
                color: Kirigami.Theme.negativeTextColor
                wrapMode: Text.WordWrap
            }
        }

        PlasmaComponents3.Label {
            visible: root.errorText !== ""
            Layout.fillWidth: true
            Layout.columnSpan: root.wideLayout ? 2 : 1
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            text: root.errorText
            color: Kirigami.Theme.negativeTextColor
            wrapMode: Text.WordWrap
        }

        Kirigami.Separator {
            Layout.fillWidth: true
            Layout.columnSpan: root.wideLayout ? 2 : 1
        }

        PlasmaComponents3.ToolButton {
            Layout.fillWidth: true
            Layout.columnSpan: root.wideLayout ? 2 : 1
            icon.name: "window-new-symbolic"
            text: "Open QuantumEngine"
            onClicked: {
                executable.connectSource("quantumengine");
                root.expanded = false;
            }
        }
    }
}
