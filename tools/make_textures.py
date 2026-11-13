#!/usr/bin/env python3
"""Draw this port's own placeholder textures (no source image is read).

The upstream textures are All Rights Reserved (upstream/LICENSE-ASSETS.md), so
every PNG under content/assets/mutant_monsters/textures/ is generated here from
seeded noise and flat shapes:

  textures/entity/mutant_zombie.png   128x128, laid out for the boxes of
                                       MutantZombieModel.createBodyLayer()
                                       (mixins/mutant_monsters/api/src/model.rs)
  textures/entity/mutant_creeper.png  128x64, laid out for the boxes of
                                       MutantCreeperModel.createBodyLayer()
                                       (mixins/mutant_monsters/api/src/creeper.rs)
  textures/entity/mutant_skeleton.png 128x128, laid out for the boxes of
                                       MutantSkeletonModel.createBodyLayer()
                                       (mixins/mutant_monsters/api/src/skeleton.rs)
  textures/item/mutant_skeleton_*.png  16x16, the five body parts
  textures/item/hulk_hammer.png        16x16
  textures/item/chemical_x.png         16x16

Run: python tools/make_textures.py   (needs Pillow)
"""
import os
import random

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
TEX = os.path.join(ROOT, "content", "assets", "mutant_monsters", "textures")

rng = random.Random(0x5EED)


def jitter(color, amount):
    return tuple(max(0, min(255, c + rng.randint(-amount, amount))) for c in color) + (255,)


def fill(img, x, y, w, h, base, amount=14):
    for yy in range(y, y + h):
        for xx in range(x, x + w):
            if 0 <= xx < img.width and 0 <= yy < img.height:
                img.putpixel((xx, yy), jitter(base, amount))


def box(img, u, v, dx, dy, dz, side, top, bottom=None, front=None):
    """Paint a model box's unwrapped faces (vanilla's ModelPart.Cube layout)."""
    bottom = bottom or top
    front = front or side
    fill(img, u + dz, v, dx, dz, top)                 # up
    fill(img, u + dz + dx, v, dx, dz, bottom)         # down
    fill(img, u, v + dz, dz, dy, side)                # right side
    fill(img, u + dz, v + dz, dx, dy, front)          # front
    fill(img, u + dz + dx, v + dz, dz, dy, side)      # left side
    fill(img, u + 2 * dz + dx, v + dz, dx, dy, side)  # back
    return (u + dz, v + dz)  # the front face's corner


SKIN = (92, 122, 66)
SKIN_DARK = (70, 96, 52)
LIMB = (78, 104, 58)
CLOTH = (52, 62, 88)
CLOTH_DARK = (40, 48, 70)


def mutant_zombie():
    img = Image.new("RGBA", (128, 128), (0, 0, 0, 0))
    # head 8x8x8 at (0, 0)
    fx, fy = box(img, 0, 0, 8, 8, 8, SKIN, SKIN_DARK)
    for ex in (fx + 1, fx + 5):          # deep-set red eyes
        fill(img, ex, fy + 3, 2, 1, (150, 20, 20), 6)
        fill(img, ex, fy + 2, 2, 1, (40, 52, 30), 4)
    fill(img, fx + 2, fy + 6, 4, 1, (30, 30, 24), 4)      # mouth
    for tx in (fx + 2, fx + 4):
        img.putpixel((tx, fy + 6), (200, 196, 170, 255))  # teeth
    # chest 24x12x16 at (0, 16): bare, with darker muscle lines and scars
    fx, fy = box(img, 0, 16, 24, 12, 16, SKIN, SKIN_DARK)
    fill(img, fx + 11, fy, 2, 12, SKIN_DARK, 6)
    fill(img, fx + 3, fy + 6, 18, 1, SKIN_DARK, 6)
    for i in range(5):
        img.putpixel((fx + 4 + i, fy + 2 + i), (120, 40, 40, 255))
    # waist 14x16x12 at (0, 44): belly on top, torn trousers below
    fx, fy = box(img, 0, 44, 14, 16, 12, SKIN, SKIN_DARK)
    fill(img, 0, 44 + 12 + 9, 2 * 12 + 2 * 14, 7, CLOTH, 10)
    for x in range(0, 52, 3):
        img.putpixel((x, 44 + 12 + 8), CLOTH_DARK + (255,))
    # arms 6x16x6 at (104, 0) and forearms at (104, 22)
    box(img, 104, 0, 6, 16, 6, LIMB, SKIN_DARK)
    box(img, 104, 22, 6, 16, 6, LIMB, SKIN_DARK)
    fill(img, 104, 22 + 6 + 13, 24, 3, (60, 80, 44), 8)  # knuckles
    # legs 6x11x6 at (80, 0) and shins at (80, 17): trousers, bare feet
    box(img, 80, 0, 6, 11, 6, CLOTH, CLOTH_DARK)
    box(img, 80, 17, 6, 8, 6, CLOTH, CLOTH_DARK, bottom=SKIN_DARK)
    fill(img, 80, 17 + 6 + 5, 24, 3, LIMB, 8)
    return img


CREEPER = (84, 150, 72)
CREEPER_DARK = (52, 104, 46)
CREEPER_PALE = (128, 176, 110)


def mottle(img, x, y, w, h, base, dark, pale, amount=16):
    """A blotchy hide: mostly `base`, with darker and paler patches."""
    for yy in range(y, y + h):
        for xx in range(x, x + w):
            r = rng.random()
            c = dark if r < 0.22 else pale if r > 0.9 else base
            img.putpixel((xx, yy), jitter(c, amount))


def creeper_box(img, u, v, dx, dy, dz):
    mottle(img, u, v, 2 * dz + 2 * dx, dz + dy, CREEPER, CREEPER_DARK, CREEPER_PALE)
    # leave the two unused corners of the unwrap clear
    for yy in range(v, v + dz):
        for xx in list(range(u, u + dz)) + list(range(u + dz + 2 * dx, u + 2 * dz + 2 * dx)):
            img.putpixel((xx, yy), (0, 0, 0, 0))
    return (u + dz, v + dz)


def mutant_creeper():
    img = Image.new("RGBA", (128, 64), (0, 0, 0, 0))
    creeper_box(img, 0, 0, 10, 14, 8)     # pelvis
    creeper_box(img, 36, 0, 9, 16, 7)     # body
    creeper_box(img, 68, 0, 8, 14, 6)     # neck
    fx, fy = creeper_box(img, 0, 22, 10, 12, 10)  # head
    # this port's own face: two slanted slits and a ragged, gaping mouth
    for i in range(3):
        img.putpixel((fx + 1 + i, fy + 3 + i // 2), (14, 18, 12, 255))
        img.putpixel((fx + 8 - i, fy + 3 + i // 2), (14, 18, 12, 255))
    fill(img, fx + 2, fy + 7, 6, 3, (20, 24, 16), 6)
    for x in (fx + 2, fx + 4, fx + 6):
        img.putpixel((x, fy + 7), CREEPER_DARK + (255,))
    for x in (fx + 3, fx + 5, fx + 7):
        img.putpixel((x, fy + 9), CREEPER_DARK + (255,))
    creeper_box(img, 40, 24, 6, 4, 14)    # front legs, upper
    creeper_box(img, 96, 0, 7, 20, 8)     # front legs, lower
    creeper_box(img, 0, 44, 4, 4, 14)     # hind legs, upper
    creeper_box(img, 80, 28, 6, 18, 6)    # hind legs, lower
    # dark feet at the bottom of the lower legs
    fill(img, 96, 8 + 17, 30, 3, (40, 70, 36), 8)
    fill(img, 80, 28 + 6 + 15, 24, 3, (40, 70, 36), 8)
    return img


BONE = (206, 200, 178)
BONE_DARK = (150, 144, 124)
BONE_PALE = (228, 224, 206)
HOLLOW = (34, 30, 28)


def mutant_skeleton():
    img = Image.new("RGBA", (128, 128), (0, 0, 0, 0))
    # skull 8x8x8 at (0, 0): eye sockets, nose hole, a crack
    fx, fy = box(img, 0, 0, 8, 8, 8, BONE, BONE_PALE, bottom=BONE_DARK)
    for ex in (fx + 1, fx + 5):
        fill(img, ex, fy + 2, 2, 2, HOLLOW, 3)
    img.putpixel((fx + 6, fy + 3), (170, 30, 30, 255))   # one ember of an eye
    fill(img, fx + 3, fy + 5, 2, 1, HOLLOW, 3)
    for i in range(3):
        img.putpixel((fx + 5 + i % 2, fy + i), BONE_DARK + (255,))
    fill(img, fx + 1, fy + 7, 6, 1, BONE_PALE, 4)        # upper teeth
    # jaw 8x3x8 at (72, 0): a row of teeth along the front
    fx, fy = box(img, 72, 0, 8, 3, 8, BONE, BONE_DARK)
    for x in range(8):
        img.putpixel((fx + x, fy), (BONE_PALE if x % 2 == 0 else BONE_DARK) + (255,))
    # pelvis 8x6x6 at (0, 16), waist 5x8x4 at (32, 0), vertebra 5x4x4 at (50, 0), neck 3x4x3 at (64, 0)
    fx, fy = box(img, 0, 16, 8, 6, 6, BONE, BONE_DARK)
    fill(img, fx + 3, fy + 2, 2, 3, HOLLOW, 3)
    fx, fy = box(img, 32, 0, 5, 8, 4, BONE_DARK, BONE)
    for y in range(1, 8, 2):
        fill(img, fx, fy + y, 5, 1, BONE, 6)
    fx, fy = box(img, 50, 0, 5, 4, 4, BONE, BONE_PALE)
    fill(img, fx, fy + 3, 5, 1, BONE_DARK, 6)
    box(img, 64, 0, 3, 4, 3, BONE_DARK, BONE)
    # rib segment 6x2x2 at (32, 12), shoulder plate 8x3x6 at (28, 16)
    box(img, 32, 12, 6, 2, 2, BONE_PALE, BONE)
    fx, fy = box(img, 28, 16, 8, 3, 6, BONE, BONE_PALE, bottom=BONE_DARK)
    fill(img, fx, fy + 2, 8, 1, BONE_DARK, 6)
    # upper arm and thigh 4x12x4 at (0, 28), forearm 4x14x4 at (16, 28), shin 4x12x4 at (32, 28)
    for u, h in ((0, 12), (16, 14), (32, 12)):
        fx, fy = box(img, u, 28, 4, h, 4, BONE, BONE_DARK)
        fill(img, fx + 1, fy + 2, 2, h - 4, BONE_DARK, 6)   # the hollow between two bones
        fill(img, u, 28 + 4, 16, 1, BONE_PALE, 4)           # knuckle at each end
        fill(img, u, 28 + 4 + h - 1, 16, 1, BONE_PALE, 4)
    return img


def bone_icon(shape):
    """A 16x16 item icon: `shape` lists (x, y) pixels, edged darker below."""
    img = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
    cells = set(shape)
    for x, y in cells:
        edge = (x, y + 1) not in cells or (x + 1, y) not in cells
        img.putpixel((x, y), jitter(BONE_DARK if edge else BONE, 8))
    return img


def skeleton_limb():
    shape = [(3 + i, 12 - i) for i in range(10)] + [(4 + i, 12 - i) for i in range(10)]
    shape += [(2, 12), (2, 13), (3, 13), (4, 14), (3, 14), (12, 2), (13, 2), (13, 3), (14, 3), (13, 1)]
    return bone_icon(shape)


def skeleton_pelvis():
    shape = [(x, y) for y in range(4, 12) for x in range(2, 14) if not (5 <= x <= 10 and 6 <= y <= 11)]
    shape += [(x, 12) for x in (2, 3, 4, 11, 12, 13)] + [(7, 3), (8, 3), (7, 2), (8, 2)]
    return bone_icon(shape)


def skeleton_rib():
    shape = []
    for i in range(11):
        y = 4 + (i * i) // 14
        shape += [(2 + i, y), (2 + i, y + 1)]
    shape += [(13, 11), (13, 12), (12, 12), (2, 3), (3, 3)]
    return bone_icon(shape)


def skeleton_shoulder_pad():
    shape = [(x, y) for y in range(5, 10) for x in range(2, 14)]
    shape += [(x, 4) for x in range(4, 12)] + [(x, 10) for x in range(3, 13)] + [(7, 11), (8, 11)]
    return bone_icon(shape)


def skeleton_skull():
    img = bone_icon([(x, y) for y in range(2, 11) for x in range(3, 13)] + [(x, y) for y in range(11, 14) for x in range(5, 11)])
    for ex in (5, 9):
        for dx in range(2):
            for dy in range(2):
                img.putpixel((ex + dx, 6 + dy), HOLLOW + (255,))
    img.putpixel((7, 9), HOLLOW + (255,))
    img.putpixel((8, 9), HOLLOW + (255,))
    for x in (6, 8):
        img.putpixel((x, 12), HOLLOW + (255,))
    return img


def hulk_hammer():
    img = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
    # handle: a diagonal from bottom left
    for i in range(10):
        x, y = 2 + i, 13 - i
        img.putpixel((x, y), jitter((104, 72, 40), 10))
        img.putpixel((x + 1, y), jitter((80, 54, 30), 8))
    # head: a heavy block across the top right, tilted with the handle
    for y in range(1, 8):
        for x in range(8, 15):
            if abs((x - 11) + (y - 4)) <= 4:
                img.putpixel((x, y), jitter((110, 112, 104), 12))
    for x, y in ((9, 2), (13, 6), (10, 3), (12, 5)):
        img.putpixel((x, y), (70, 72, 66, 255))
    return img


def chemical_x():
    img = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
    glass = (200, 220, 230, 255)
    for y in range(6, 15):
        for x in range(4, 12):
            edge = x in (4, 11) or y == 14
            if edge:
                img.putpixel((x, y), glass)
            elif y >= 8:
                img.putpixel((x, y), jitter((60, 200, 60), 30))
    for y in range(2, 6):          # neck
        img.putpixel((6, y), glass)
        img.putpixel((9, y), glass)
    for x in range(6, 10):         # cork
        img.putpixel((x, 1), (120, 84, 50, 255))
    for x, y in ((6, 9), (9, 9), (7, 10), (8, 10), (7, 11), (8, 11), (6, 12), (9, 12)):
        img.putpixel((x, y), (20, 60, 20, 255))  # an X in the liquid
    return img


def main():
    os.makedirs(os.path.join(TEX, "entity"), exist_ok=True)
    os.makedirs(os.path.join(TEX, "item"), exist_ok=True)
    mutant_zombie().save(os.path.join(TEX, "entity", "mutant_zombie.png"))
    mutant_creeper().save(os.path.join(TEX, "entity", "mutant_creeper.png"))
    mutant_skeleton().save(os.path.join(TEX, "entity", "mutant_skeleton.png"))
    for name, draw in (
        ("limb", skeleton_limb),
        ("pelvis", skeleton_pelvis),
        ("rib", skeleton_rib),
        ("shoulder_pad", skeleton_shoulder_pad),
        ("skull", skeleton_skull),
    ):
        draw().save(os.path.join(TEX, "item", "mutant_skeleton_" + name + ".png"))
    hulk_hammer().save(os.path.join(TEX, "item", "hulk_hammer.png"))
    chemical_x().save(os.path.join(TEX, "item", "chemical_x.png"))
    print("textures written to", TEX)


if __name__ == "__main__":
    main()
