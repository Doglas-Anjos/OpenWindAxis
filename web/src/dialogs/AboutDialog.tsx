/** Help › About: the product, its version and what it is built on. */
import { Logo } from '../shell/icons';
import { PRODUCT_NAME, PRODUCT_TAGLINE, PRODUCT_VERSION } from '../shell/brand';
import { Modal } from './Modal';

/** Size of the mark in the dialog, px. */
const LOGO_SIZE = 48;

export function AboutDialog({ onClose }: { onClose: () => void }) {
  return (
    <Modal
      title={`Sobre o ${PRODUCT_NAME}`}
      onClose={onClose}
      buttons={
        <button className="btn primary" onClick={onClose}>
          OK
        </button>
      }
    >
      <div className="about">
        <Logo size={LOGO_SIZE} />
        <div>
          <h2>{PRODUCT_NAME}</h2>
          <p>{PRODUCT_TAGLINE}</p>
          <p className="muted">Versão {PRODUCT_VERSION}</p>
          <p>
            Núcleo de cálculo em Rust, executado no navegador como WebAssembly: os dados não saem do seu computador. Os mapas do explorador vêm do
            OpenStreetMap (© colaboradores do OpenStreetMap), exibidos com Leaflet; os gráficos usam Apache ECharts.
          </p>
          <p className="muted">
            Desenvolvido em sala limpa: a organização da interface segue convenções de ferramentas de recurso eólico, com nome, ícones, cores e código
            próprios.
          </p>
        </div>
      </div>
    </Modal>
  );
}
