//! Regenerates intrinsic material maps into a new directory for review or publication.
use std::{
    fs,
    path::{Path, PathBuf},
};
use superspec_engine::{
    materialspec::authoring::{Maps, concrete, intrinsic::brushed_metal, timber},
    worldspec::themes::kowloon::authoring::intrinsic::{kitchen, restaurant_tiles, vent},
};
use superspec_geometry::image::{self, Image};
#[path = "materials/appearances.rs"]
mod appearances;
#[path = "materials/atlas.rs"]
mod atlas;
#[path = "materials/bags.rs"]
mod bags;
#[path = "materials/clean_stock.rs"]
mod clean_stock;
#[path = "materials/data_textures.rs"]
mod data_textures;
#[path = "materials/kit.rs"]
mod kit;
#[path = "materials/liquor_meshes.rs"]
mod liquor_meshes;
#[path = "../material_library.rs"]
mod material_library;
#[path = "materials/prop_textures.rs"]
mod prop_textures;
#[path = "materials/refine.rs"]
mod refine;
#[path = "materials/site.rs"]
mod site;
#[path = "materials/stock.rs"]
mod stock;
#[path = "materials/stock_meshes.rs"]
mod stock_meshes;

/// Offline regeneration reads each stage's updated catalogs from the authored distribution.
fn shop_catalog_directory() -> Result<PathBuf, String> {
    let path = std::env::var_os("SUPERSPEC_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .join("crates/engine/src/worldspec/themes/kowloon/shop/assets");
    if !path.is_dir() {
        return Err(format!(
            "Offline catalog inputs are missing at {}. Set SUPERSPEC_ROOT to a SuperSpec source checkout; authored source art remains in the supplied LevelSpec project root.",
            path.display()
        ));
    }
    Ok(path)
}

trait WriteMaps {
    fn write(self, out: &Path, name: &str) -> Result<(), String>;
}
impl WriteMaps for Maps {
    fn write(self, out: &Path, name: &str) -> Result<(), String> {
        let files = self.encode(name)?;
        fs::create_dir(out).map_err(|e| e.to_string())?;
        for (name, bytes) in files {
            fs::write(out.join(name), bytes).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

fn decode(path: &Path) -> Result<Image, String> {
    image::decode(&fs::read(path).map_err(|e| e.to_string())?)
}

fn inputs() -> superspec_engine::worldspec::themes::kowloon::authoring::Inputs<'static> {
    fn read(path: &Path) -> Result<Vec<u8>, String> {
        fs::read(path).map_err(|e| e.to_string())
    }
    fn exists(path: &Path) -> bool {
        path.exists()
    }
    fn catalog(name: &str) -> Result<Vec<u8>, String> {
        read(&shop_catalog_directory()?.join(name))
    }
    superspec_engine::worldspec::themes::kowloon::authoring::Inputs {
        read: &read,
        exists: &exists,
        catalog: &catalog,
    }
}
fn publish(out: &Path, files: Vec<(String, Vec<u8>)>) -> Result<(), String> {
    fs::create_dir(out).map_err(|e| e.to_string())?;
    for (name, bytes) in files {
        fs::write(out.join(name), bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn kit_textures(output: &Path, args: &[std::ffi::OsString]) -> Result<(), String> {
    let mut args = args.iter();
    let mut font_dir = None;
    let mut extra = "";
    let mut extend = None;
    let mut names = Vec::new();
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--fonts") => {
                font_dir = Some(Path::new(args.next().ok_or("Missing font directory.")?))
            }
            Some("--extra") => {
                extra = args
                    .next()
                    .and_then(|arg| arg.to_str())
                    .ok_or("Missing UTF-8 extra glyphs.")?
            }
            Some("--extend") => {
                extend = Some(Path::new(
                    args.next().ok_or("Missing shipped atlas image.")?,
                ))
            }
            Some(name) => names.push(name.to_owned()),
            None => return Err("Texture name must be valid UTF-8.".into()),
        }
    }
    kit::bake(output, font_dir, &names, extra, extend)
}

const USAGE: &str = "\
Usage: superspec-materials RECIPE INPUTS OUTPUT
  site-textures | kowloon-prop-textures | kitchen-metal | brushed-metal | fixture-finishes | grille-frame | casement-finishes  NEW_DIRECTORY
  kowloon-kit-textures  NEW_DIRECTORY [TEXTURE ...] [--fonts DIRECTORY] [--extra CHARACTERS] [--extend LETTERING_PNG]
  concrete  SOURCE_A SOURCE_B NEW_DIRECTORY
  vent-metal | restaurant-tiles | shop-timber  SOURCE_PNG NEW_DIRECTORY
  crate-wear | shop-cartons  REPOSITORY [ATLAS_DIRECTORY] NEW_DIRECTORY
  stock-appearances | clean-stock | snack-packets  REPOSITORY ATLAS_DIRECTORY NEW_DIRECTORY
  refine-stock  REPOSITORY RAW_BAKE_CATALOG NEW_DIRECTORY
  pack-liquor | pack-restaurant-food  BAKE_DIRECTORY NEW_DIRECTORY
  pack-balcony  BAKE_DIRECTORY COTTON_PNG NEW_DIRECTORY
  extract-glb-maps  SOURCE_GLB ASSET_NAME OUTPUT_DIRECTORY
  bag-clusters | prepare-liquor-meshes | finish-liquor-meshes | apple-pile  SOURCE_JSON OUTPUT_JSON
  repair-snack-packet  SOURCE_PNG ASPECT_RATIO NEW_DIRECTORY
  encode-city-data-textures  MANIFEST_JSON
  resize-stock-texture  SOURCE_IMAGE MAX_EDGE OUTPUT_PNG";

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [name, manifest] if name == "encode-city-data-textures" => {
            data_textures::run(Path::new(manifest))
        }
        [name, source, asset, output] if name == "extract-glb-maps" => atlas::extract_maps(
            Path::new(source),
            asset.to_str().ok_or("Invalid asset name.")?,
            Path::new(output),
        ),
        [name] if name == "--help" || name == "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        [name, output, options @ ..] if name == "kowloon-kit-textures" => {
            kit_textures(Path::new(output), options)
        }
        [name, output] if name == "kowloon-prop-textures" => prop_textures::bake(Path::new(output)),
        [name, output] if name == "site-textures" => site::bake(Path::new(output)),
        [name, source, output] if name == "finish-liquor-meshes" => {
            liquor_meshes::finish(Path::new(source), Path::new(output))
        }
        [name, source, output] if name == "prepare-liquor-meshes" => {
            liquor_meshes::prepare(Path::new(source), Path::new(output))
        }
        [name, source, output] if name == "apple-pile" => {
            stock_meshes::apples(Path::new(source), Path::new(output))
        }
        [name, source, edge, output] if name == "resize-stock-texture" => {
            let edge = edge
                .to_str()
                .and_then(|arg| arg.parse().ok())
                .ok_or("Invalid texture edge.")?;
            stock_meshes::resize(Path::new(source), edge, Path::new(output))
        }
        [name, source, aspect, output] if name == "repair-snack-packet" => {
            let aspect = aspect
                .to_str()
                .and_then(|arg| arg.parse().ok())
                .ok_or("Invalid packet aspect ratio.")?;
            stock_meshes::packet(Path::new(source), aspect, Path::new(output))
        }
        [name, root, catalog, output] if name == "refine-stock" => {
            refine::bake(Path::new(root), Path::new(catalog), Path::new(output))
        }
        [name, root, textures, output]
            if name == "stock-appearances" || name == "snack-packets" =>
        {
            appearances::bake(
                Path::new(root),
                Path::new(textures),
                Path::new(output),
                name == "snack-packets",
            )
        }
        [name, root, textures, output] if name == "clean-stock" => {
            clean_stock::bake(Path::new(root), Path::new(textures), Path::new(output))
        }
        [name, root, textures, output] if name == "crate-wear" || name == "shop-cartons" => {
            stock::bake_from(
                name == "shop-cartons",
                Path::new(root),
                Path::new(textures),
                Path::new(output),
            )
        }
        [name, source, output] if name == "bag-clusters" => {
            bags::bake(Path::new(source), Path::new(output))
        }
        [name, source, cotton, output] if name == "pack-balcony" => atlas::balcony(
            Path::new(source),
            &decode(Path::new(cotton))?,
            Path::new(output),
        ),
        [name, source, output] if name == "pack-liquor" || name == "pack-restaurant-food" => {
            atlas::pack(
                Path::new(source),
                Path::new(output),
                name == "pack-restaurant-food",
            )
        }
        [name, output] if name == "grille-frame" => publish(
            Path::new(output),
            superspec_engine::worldspec::themes::kowloon::authoring::grille_frame::bake()?,
        ),
        [name, output] if name == "casement-finishes" => publish(
            Path::new(output),
            superspec_engine::worldspec::themes::kowloon::authoring::casement_finishes::bake()?,
        ),
        [name, output] if name == "fixture-finishes" => publish(
            Path::new(output),
            superspec_engine::worldspec::themes::kowloon::authoring::fixture_finishes::bake()?,
        ),
        [name, output] if name == "brushed-metal" => {
            brushed_metal().write(Path::new(output), "brushed-metal")
        }
        [name, first, second, output] if name == "concrete" => {
            concrete::bake(&decode(Path::new(first))?, &decode(Path::new(second))?)?
                .write(Path::new(output), "concrete-shared")
        }
        [name, root, output] if name == "crate-wear" || name == "shop-cartons" => {
            stock::bake(name == "shop-cartons", Path::new(root), Path::new(output))
        }
        [name, output] if name == "kitchen-metal" => {
            kitchen().write(Path::new(output), "kitchen-metal")
        }
        [name, source, output] if name == "shop-timber" => {
            timber::bake(&decode(Path::new(source))?)?.write(Path::new(output), "shop-timber")
        }
        [name, source, output] if name == "vent-metal" => {
            vent(&decode(Path::new(source))?)?.write(Path::new(output), "vent-metal")
        }
        [name, source, output] if name == "restaurant-tiles" => {
            restaurant_tiles(&decode(Path::new(source))?)?
                .write(Path::new(output), "restaurant-tiles")
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
