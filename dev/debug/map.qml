import QtQuick
import QtQuick.Controls
import QtLocation
import QtPositioning

ApplicationWindow {
    visible: true
    width: 800
    height: 600

    Map {
        id: map
        anchors.fill: parent
        plugin: Plugin { name: "osm" }

        center: QtPositioning.coordinate(55.705, 13.191)
        zoomLevel: 13

        // 🔴 A simple red point
        MapQuickItem {
            coordinate: QtPositioning.coordinate(55.705, 13.191)
            anchorPoint.x: 6
            anchorPoint.y: 6

            sourceItem: Rectangle {
                width: 12
                height: 12
                radius: 6
                color: "red"
                border.color: "black"
            }
        }
    }
}