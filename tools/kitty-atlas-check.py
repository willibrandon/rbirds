"""Check live sprite geometry using Kitty's own protocol decoder, without a GUI."""
from pathlib import Path
import hashlib
import json

from kitty.fast_data_types import Screen, set_options
from kitty.options.types import defaults

set_options(defaults)


class Callbacks:
    def __getattr__(self, name):
        return lambda *args: None


def parse(screen, data):
    data = memoryview(data)
    while data:
        destination = screen.test_create_write_buffer()
        count = screen.test_commit_write_buffer(data, destination)
        data = data[count:]
        screen.test_parse_written_data()


def decode(path):
    callbacks = Callbacks()
    screen = Screen(callbacks, 32, 100, 0, 14, 25, 0, callbacks)
    parse(screen, path.read_bytes())
    manager = screen.grman
    layers = manager.update_layers(0, -1., 1., 2 / 100, 2 / 32, 100, 32, 14, 25)
    images = {}
    for number in range(1, 3000):
        image = manager.image_for_client_number(number)
        if image:
            images[image["internal_id"]] = image
    result = []
    for index, layer in enumerate(layers):
        group = layers[index:index + layer["group_count"]]
        # Kitty draws negative and positive z in separate passes. A shared
        # image group must finish before the text boundary.
        assert all((member["z_index"] < 0) == (layer["z_index"] < 0)
                   for member in group), (path, index, "image group crosses text layer")
        image = images[layer["image_id"]]
        width, height = image["width"], image["height"]
        channels = len(image["data"]) // width // height
        rect = layer["src_rect"]
        left, top, right, bottom = [round(rect[key] * size) for key, size in
                                   [("left", width), ("top", height),
                                    ("right", width), ("bottom", height)]]
        pixels = b"".join(image["data"][(y * width + left) * channels:
                                      (y * width + right) * channels]
                          for y in range(top, bottom))
        if channels == 3:
            pixels = b"".join(pixels[i:i + 3] + b"\xff" for i in range(0, len(pixels), 3))
        result.append({
            "rect": [round(layer["dest_rect"][key], 6)
                     for key in ["left", "top", "right", "bottom"]],
            "width": right - left, "height": bottom - top,
            "pixels": hashlib.sha256(pixels).hexdigest(),
            "under_text": layer["z_index"] < 0,
        })
    return result, manager.image_count


for size in [4, 30, 64]:
    root = Path("target/kitty-atlas-check")
    before, nb = decode(root / f"scene-before-{size}.bin")
    after, na = decode(root / f"scene-after-{size}.bin")
    assert len(before) == len(after) == 364, (size, len(before), len(after))
    assert before == after, (size, [(i, b, a) for i, (b, a) in enumerate(zip(before, after)) if b != a][:1])
    print(json.dumps(dict(size=size, placements=len(before), images_before=nb, images_after=na,
                          identical_source_pixels_positions_and_order=True)))
