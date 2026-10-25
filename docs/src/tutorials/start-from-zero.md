# Start from zero

<p class="kp-lede">You have never made a game, and maybe never installed a program by hand. That
is fine. By the end of this page you will have Khora on your computer, a project
of your own open in the editor, and a cube moving on screen — about twenty
minutes, most of it waiting for a download.</p>

A **game engine** is the program that does the hard, invisible work of a game:
drawing the picture sixty times a second, making things fall and bounce, playing
sounds, reading the keyboard. You bring the ideas — the world, the characters,
the rules — and the engine runs them. Khora is one of these engines.

## What you need

- **A computer** running Windows 10 or 11, Linux, or a Mac with an Apple chip
  (M1 or later).
- **A graphics card from the last ten years** or so. Almost every computer has
  one built in; Khora talks to it through Vulkan, DirectX 12 or Metal.
- **About 2 GB of free space**, and an internet connection for the download.

You do **not** need to know how to program. You do not need to install anything
else.

## 1. Download the Hub

The **Hub** is a small program that installs Khora and keeps your projects in
one place.

1. Open the [Khora releases page](https://github.com/eraflo/KhoraEngine/releases).
   *GitHub* is the website where Khora's code lives; a *release* is a version
   packaged for you to download.
2. Under the newest release, open **Assets** and download the Hub for your
   computer:

   | Your computer | The file to download |
   |---|---|
   | Windows | `khora-hub-windows-x86_64.zip` |
   | Linux | `khora-hub-linux-x86_64.tar.gz` |
   | Mac (Apple chip) | `khora-hub-macos-aarch64.tar.gz` |

3. The file is an **archive** — a box holding several files. Extract it: on
   Windows, right-click it and choose **Extract All…**; on a Mac, double-click
   it. You get a folder with the Hub inside.
4. Open the Hub: `khora-hub.exe` on Windows, `khora-hub` elsewhere.

   Windows may warn that it "protected your PC" from an app it does not know
   yet. Click **More info**, then **Run anyway**.

## 2. Install the engine

The Hub can hold several versions of Khora side by side; you need one.

1. In the Hub, open **Engines**.
2. Under **Available releases**, click **Download** next to the newest one.
   Wait until it moves to **Installed**.

## 3. Create your project

A **project** is a folder holding everything your game is made of: its scenes,
its pictures and sounds, its scripts.

1. Go back to **Projects** and click **New project**.
2. Give it a **Project name** — `MyFirstGame` will do.
3. Choose the **Parent directory**: the folder your project's folder will be
   made in. Your *Documents* folder is a good choice.
4. Under **Engine**, pick the version you just installed.
5. Leave the git options as they are, and click **Create project**.

The editor opens on your new project.

## 4. Find your way around the editor

The editor is where you build your game. Five places matter for now:

| Place | What it is for |
|---|---|
| **Viewport** (the big picture in the middle) | your game's world, seen through a camera you can move: hold the right mouse button and move the mouse to turn around what you look at, roll the mouse wheel to come closer, hold `Shift` and the wheel button to slide sideways |
| **Scene Tree** (left) | every thing in the world, in a list — a camera and a light to begin with |
| **Inspector** (right) | the details of the thing you selected: where it is, what it looks like, what it does |
| **Assets** (bottom) | your project's files: pictures, sounds, meshes, scripts |
| **Console** (bottom, beside Assets) | messages from the engine and from your scripts |

## 5. Put something in the world, and play

1. In the **Scene Tree**, right-click the empty space under the light and
   choose **Add → Cube**. A cube appears in the viewport.
2. With the cube selected, drag the coloured arrows on it to move it. Each arrow
   is one direction: red left–right, green up–down, blue forward–back.
3. Press **Play** at the top of the window. The game runs. Nothing moves yet —
   nothing has been told to.
4. Press **Stop**. You are back to building.

## Where next

- **Make the cube move by itself** — [Scripting](../concepts/scripting/index.md)
  begins with what a script is, then has you write one in five steps.
- **Dress the world** — [Materials and lighting](../how-to/materials-and-lighting.md).
- **Bring your own pictures and models** — [Load assets](../how-to/load-assets.md).

When a word in these pages is new, the [Glossary](../reference/glossary.md)
says what it means in a sentence or two.
