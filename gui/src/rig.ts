// Builds a GoldSrc model as three.js objects and poses it per frame (H4).
// Vertices sit in their bone's space, so each corner is a skinned vertex
// with weight 1 on its bone and identity bind matrices: world = bone · p.
import * as THREE from "three";
import * as api from "./api";
import type { ModelView } from "./api";

export interface Rig {
  view: ModelView;
  bones: THREE.Bone[];
  meshes: THREE.SkinnedMesh[];
  hitboxes: THREE.Object3D[];
  dispose(): void;
}

/** Load the converted model (or the weapon) with its textures. `attachTo`
 *  bone-merges by name onto another rig, as Sven's weapon models do. */
export async function build(parent: THREE.Object3D, weapon: boolean, attachTo?: Rig): Promise<Rig> {
  const view = await api.modelView(weapon);
  const rest = await api.boneFrames(weapon, view.generation, null, 0);

  const bones = view.bones.map((b) => {
    const bone = new THREE.Bone();
    bone.name = b.name;
    bone.matrixAutoUpdate = false;
    return bone;
  });
  view.bones.forEach((b, i) => {
    const merged = attachTo?.bones.find((x) => x.name === b.name);
    if (merged) {
      merged.add(bones[i]); // identity: follows the player's bone
    } else {
      setLocal(bones[i], rest, i);
      (b.parent >= 0 ? bones[b.parent] : parent).add(bones[i]);
    }
  });

  const textures = await Promise.all(
    view.textures.map(async (t, i) => {
      const tex = new THREE.DataTexture(
        await api.texture(weapon, view.generation, i),
        t.width,
        t.height,
        THREE.RGBAFormat,
      );
      tex.colorSpace = THREE.SRGBColorSpace;
      tex.magFilter = THREE.LinearFilter;
      tex.minFilter = THREE.LinearMipmapLinearFilter;
      tex.generateMipmaps = true;
      tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
      tex.needsUpdate = true;
      return tex;
    }),
  );

  const skeleton = new THREE.Skeleton(bones, bones.map(() => new THREE.Matrix4()));
  const meshes = view.meshes.map((m) => {
    const g = new THREE.BufferGeometry();
    const n = m.bones.length;
    g.setAttribute("position", new THREE.Float32BufferAttribute(m.positions, 3));
    g.setAttribute("normal", new THREE.Float32BufferAttribute(m.normals, 3));
    g.setAttribute("uv", new THREE.Float32BufferAttribute(m.uvs, 2));
    const idx = new Uint16Array(n * 4);
    const w = new Float32Array(n * 4);
    for (let i = 0; i < n; i++) {
      idx[i * 4] = m.bones[i];
      w[i * 4] = 1;
    }
    g.setAttribute("skinIndex", new THREE.Uint16BufferAttribute(idx, 4));
    g.setAttribute("skinWeight", new THREE.Float32BufferAttribute(w, 4));
    const t = view.textures[m.texture];
    const mat = new THREE.MeshLambertMaterial({
      map: textures[m.texture] ?? null,
      alphaTest: t?.masked ? 0.5 : 0,
      side: THREE.FrontSide, // back faces culled, as in game
    });
    const mesh = new THREE.SkinnedMesh(g, mat);
    mesh.frustumCulled = false; // bind positions are bone-local
    parent.add(mesh);
    mesh.bind(skeleton, new THREE.Matrix4());
    return mesh;
  });

  const hitboxes = view.hitboxes.map((h) => {
    const size = [0, 1, 2].map((k) => h.max[k] - h.min[k]);
    const box = new THREE.LineSegments(
      new THREE.EdgesGeometry(new THREE.BoxGeometry(size[0], size[1], size[2])),
      new THREE.LineBasicMaterial({ color: 0xff3b30, depthTest: false, transparent: true, opacity: 0.8 }),
    );
    box.position.set(...([0, 1, 2].map((k) => (h.max[k] + h.min[k]) / 2) as [number, number, number]));
    box.renderOrder = 2;
    bones[h.bone]?.add(box);
    return box;
  });

  return {
    view,
    bones,
    meshes,
    hitboxes,
    dispose() {
      for (const m of meshes) {
        m.removeFromParent();
        m.geometry.dispose();
        (m.material as THREE.Material).dispose();
      }
      textures.forEach((t) => t.dispose());
      for (const h of hitboxes) {
        (h as THREE.LineSegments).geometry.dispose();
      }
      bones.forEach((b) => b.removeFromParent());
      skeleton.dispose();
    },
  };
}

/** Set bone `i`'s local transform from a frame block (12 floats per bone). */
export function setLocal(bone: THREE.Bone, frame: Float32Array, i: number, offset = 0) {
  const a = frame.subarray(offset + i * 12, offset + i * 12 + 12);
  bone.matrix.set(a[0], a[1], a[2], a[9], a[3], a[4], a[5], a[10], a[6], a[7], a[8], a[11], 0, 0, 0, 1);
  bone.matrixWorldNeedsUpdate = true;
}
