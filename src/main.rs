use num::pow;
use osmio::obj_types::RcNode;
use osmio::{Node, OSMObj, OSMObjBase, OSMReader};
use serde::Deserialize;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use xml;
use xml::writer::XmlEvent;

#[derive(Debug, Deserialize)]
struct Peak {
    name: String,
    lat: f32,
    lon: f32,
    ele: f32,
}

fn pb_peaks() -> csv::Result<Vec<Peak>> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter('|' as u8)
        .has_headers(false)
        .from_path("/Users/nhowell/gis/SierraExtract.txt")?;

    reader
        .deserialize()
        .collect::<csv::Result<Vec<Peak>>>()
        .map(|peaks| {
            let mut p = peaks
                .into_iter()
                .map(|peak| Peak {
                    ele: peak.ele * 0.3048, // convert to meters
                    ..peak
                })
                .collect::<Vec<Peak>>();
            p.sort_by_cached_key(|x| x.name.clone());
            p
        })
}

fn osm_peaks() -> Result<Vec<RcNode>, Box<dyn Error>> {
    let file = File::open("/Users/nhowell/gis/california-peaks.osm.pbf")?;
    let buf_reader = BufReader::new(file);
    let mut pbf = osmio::pbf::PBFReader::new(buf_reader);
    Ok(pbf
        .objects()
        .filter_map(|x| x.into_node())
        .filter(|x| x.tag("natural") == Some("peak"))
        .collect())
}

fn point(lat: f32, lon: f32) -> (f32, f32) {
    let (n, e, _) = utm::to_utm_wgs84(lat as f64, lon as f64, 11);
    (n as f32, e as f32)
}

fn main() -> Result<(), Box<dyn Error>> {
    let osm_peaks = osm_peaks()?;
    let mut index = kdtree::KdTree::with_capacity(2, osm_peaks.len());
    for peak in osm_peaks {
        if !peak.has_lat_lon() {
            continue;
        }

        let (lat, lon) = peak.lat_lon().unwrap();
        let (northing, easting) = point(lat, lon);
        index.add([northing, easting], peak)?;
    }

    let target = File::create("/tmp/peaks.osm")?;
    let mut writer: xml::EventWriter<_> = xml::EmitterConfig::new()
        .perform_indent(true)
        .create_writer(target);
    writer.write(xml::writer::XmlEvent::start_element("osm").attr("version", "0.6"))?;
    for pb_peak in pb_peaks()? {
        let (northing, easting) = point(pb_peak.lat, pb_peak.lon);
        let nearby = index.within(
            &[northing, easting],
            pow(100.0f32, 2), // squared to match metric function
            &kdtree::distance::squared_euclidean,
        )?;

        for (dist, osm_peak) in nearby {
            if osm_peak.tag("name") != Some(&pb_peak.name) {
                continue;
            }

            if !osm_peak.has_tag("ele") {
                continue;
            }

            let dist = dist.sqrt();
            if dist < 75f32 {
                continue;
            }

            let version = osm_peak.version().unwrap();

            writer.write(
                XmlEvent::start_element("node")
                    .attr("id", osm_peak.id().to_string().as_str())
                    .attr("action", "modify")
                    .attr("version", version.to_string().as_str())
                    .attr("lat", pb_peak.lat.to_string().as_str())
                    .attr("lon", pb_peak.lon.to_string().as_str()),
            )?;

            for (k, v) in osm_peak.tags() {
                writer.write(XmlEvent::start_element("tag").attr("k", k).attr("v", v))?;
                writer.write(XmlEvent::end_element())?;
            }

            // let ele = pb_peak.ele as u32;
            // writer.write(
            //     XmlEvent::start_element("tag")
            //         .attr("k", "ele")
            //         .attr("v", ele.to_string().as_str()),
            // )?;
            // writer.write(XmlEvent::end_element())?;

            writer.write(XmlEvent::end_element())?;

            println!("{} {:?}", dist, osm_peak);
        }
    }
    writer.write(xml::writer::XmlEvent::end_element())?;

    Ok(())
}
